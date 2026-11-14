//! "Check my environment": looks at the whole setup and says, in plain language, what is wrong and how to fix it.
//! Read-only on purpose (no file is written, no program is started), so it is safe to run at any time.
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use crate::settings::{ServiceCfg, Settings};
use crate::{certs, phpini, ports, services, sites};

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Level {
    Ok,
    Warn,
    Error,
}

#[derive(Clone, Debug)]
pub struct Finding {
    pub level: Level,
    pub title: String,
    pub detail: String,
    /// What to do about it (None when there is nothing to do).
    pub fix: Option<String>,
}

/// How many findings are Ok / Warn / Error, for a one-line summary.
pub fn counts(list: &[Finding]) -> (usize, usize, usize) {
    let n = |l: Level| list.iter().filter(|f| f.level == l).count();
    (n(Level::Ok), n(Level::Warn), n(Level::Error))
}

fn mk(level: Level, title: impl Into<String>, detail: impl Into<String>, fix: Option<&str>) -> Finding {
    Finding { level, title: title.into(), detail: detail.into(), fix: fix.map(str::to_string) }
}
fn ok(title: impl Into<String>, detail: impl Into<String>) -> Finding {
    mk(Level::Ok, title, detail, None)
}
fn warn(title: impl Into<String>, detail: impl Into<String>, fix: &str) -> Finding {
    mk(Level::Warn, title, detail, Some(fix))
}
fn err(title: impl Into<String>, detail: impl Into<String>, fix: &str) -> Finding {
    mk(Level::Error, title, detail, Some(fix))
}

/// Runs every check (no changes made). Takes a few seconds (netstat, httpd -t, mysql...); call it on a thread.
pub fn check(s: &Settings) -> Vec<Finding> {
    let mut out = Vec::new();
    // One look at the ports for all checks, so the servers are compared against the same picture.
    let listeners = ports::listening();
    guarded("Visual C++", &mut out, |o| vc_runtime(s, o));
    guarded("Servidores e portas", &mut out, |o| servers(s, &listeners, o));
    guarded("Apache", &mut out, |o| apache(s, o));
    guarded("PHP", &mut out, |o| php(s, o));
    guarded("MySQL", &mut out, |o| mysql(s, &listeners, o));
    guarded("Sites", &mut out, |o| site_folder(s, o));
    guarded("Arquivo hosts", &mut out, |o| hosts(s, o));
    guarded("HTTPS", &mut out, |o| https(s, o));
    guarded("Disco", &mut out, |o| disk(s, o));
    out
}

/// One check never takes the others down: a panic inside it becomes a finding, and what it found before stays.
fn guarded(name: &str, out: &mut Vec<Finding>, f: impl FnOnce(&mut Vec<Finding>)) {
    let mut local = Vec::new();
    let r = catch_unwind(AssertUnwindSafe(|| f(&mut local)));
    out.append(&mut local);
    if r.is_err() {
        out.push(warn(format!("Não consegui verificar: {name}"), "A verificação falhou de forma inesperada.", "Tente de novo; se repetir, veja a atividade do Ninho."));
    }
}

/// Runs a program and returns (success, stderr + stdout). None when it cannot even start.
fn run(exe: &Path, args: &[&str], cwd: Option<&Path>) -> Option<(bool, String)> {
    let mut c = Command::new(exe);
    c.args(args).stdin(Stdio::null());
    if let Some(d) = cwd {
        c.current_dir(d);
    }
    services::hide_window(&mut c);
    let o = c.output().ok()?;
    // Apache prints "Syntax OK" on stderr, PHP prints its version on stdout: keep both.
    let mut text = String::from_utf8_lossy(&o.stderr).into_owned();
    text.push('\n');
    text.push_str(&String::from_utf8_lossy(&o.stdout));
    Some((o.status.success(), text))
}

// ---------------------------------------------------------------- Visual C++

/// Apache, PHP and PostgreSQL builds for Windows are linked against the 2015-2022 runtime; without it they die
/// at start with a missing-DLL popup, which is the most common "nothing starts" cause.
fn vc_runtime(s: &Settings, out: &mut Vec<Finding>) {
    let win = std::env::var("SystemRoot").unwrap_or_else(|_| r"C:\Windows".into());
    let sys = Path::new(&win).join("System32");
    let missing: Vec<&str> = ["vcruntime140.dll", "msvcp140.dll"].into_iter().filter(|f| !sys.join(f).is_file()).collect();
    let needed = ["apache", "php", "postgres"].iter().any(|k| services::version_dir(s, k).is_some());
    if missing.is_empty() {
        out.push(ok("Visual C++ 2015-2022 instalado", "O Apache, o PHP e o PostgreSQL precisam dele e ele está presente."));
        return;
    }
    let detail = format!("Faltam no Windows: {}. Sem eles o Apache, o PHP e o PostgreSQL não abrem.", missing.join(", "));
    let fix = "Instale o \"Visual C++ Redistributable 2015-2022 (x64)\": https://aka.ms/vs/17/release/vc_redist.x64.exe";
    out.push(if needed { err("Falta o Visual C++ 2015-2022", detail, fix) } else { warn("Falta o Visual C++ 2015-2022", detail, fix) });
}

// ---------------------------------------------------------------- servers and ports

#[derive(Debug, PartialEq, Eq)]
pub(crate) enum PortStatus {
    Free,
    /// Held by the program of its own kind (the server is running).
    Own,
    Other,
}

/// Same rule as `Shared::refresh`: the right program name, or a name Windows did not tell, counts as ours.
pub(crate) fn port_status(holder: Option<&str>, image: &str) -> PortStatus {
    match holder {
        None => PortStatus::Free,
        Some(p) if p.is_empty() || p.eq_ignore_ascii_case(image) => PortStatus::Own,
        Some(_) => PortStatus::Other,
    }
}

/// Ports Windows keeps for itself (Hyper-V, WSL, Docker...), from `netsh int ipv4 show excludedportrange`.
/// A line is "  start   end" (a "*" after a number marks an administrator exclusion).
pub(crate) fn parse_excluded(text: &str) -> Vec<(u16, u16)> {
    text.lines()
        .filter_map(|l| {
            let mut it = l.split_whitespace().filter(|w| *w != "*").map(|w| w.trim_end_matches('*'));
            let (a, b) = (it.next()?.parse().ok()?, it.next()?.parse().ok()?);
            it.next().is_none().then_some((a, b))
        })
        .collect()
}

fn excluded_ranges() -> Vec<(u16, u16)> {
    let mut c = Command::new("netsh");
    c.args(["int", "ipv4", "show", "excludedportrange", "protocol=tcp"]);
    services::hide_window(&mut c);
    c.output().map(|o| parse_excluded(&String::from_utf8_lossy(&o.stdout))).unwrap_or_default()
}

/// Servers (enabled ones) that were given the same port: only one of them can start.
pub(crate) fn duplicate_ports(list: &[(String, u16)]) -> Vec<(u16, Vec<String>)> {
    let mut out: Vec<(u16, Vec<String>)> = Vec::new();
    for (name, port) in list {
        match out.iter_mut().find(|(p, _)| p == port) {
            Some(e) => e.1.push(name.clone()),
            None => out.push((*port, vec![name.clone()])),
        }
    }
    out.retain(|(_, v)| v.len() > 1);
    out
}

fn port_finding(name: &str, port: u16, image: &str, listeners: &[ports::Listener], reserved: &[(u16, u16)]) -> Finding {
    let holder = ports::owner(listeners, port);
    match port_status(holder.map(|l| l.program.as_str()), image) {
        PortStatus::Own => ok(format!("{name}: porta {port} em uso por ele mesmo"), "O servidor está rodando."),
        PortStatus::Free if reserved.iter().any(|&(a, b)| (a..=b).contains(&port)) => err(
            format!("{name}: a porta {port} está reservada pelo Windows"),
            "O Windows separou essa faixa de portas (Hyper-V, WSL ou Docker costumam fazer isso) e nenhum programa consegue abri-la.",
            "Escolha outra porta para este servidor em Configurações.",
        ),
        PortStatus::Free => ok(format!("{name}: porta {port} livre"), "Nada está usando essa porta."),
        PortStatus::Other => {
            let l = holder.expect("Other has a holder");
            let who = if l.program.is_empty() { format!("PID {}", l.pid) } else { format!("{} (PID {})", l.program, l.pid) };
            let sys = ports::is_system(&l.program, l.pid);
            let detail = if sys {
                format!("A porta {port} está nas mãos de {who}, que é parte do próprio Windows (portas baixas como a 1025 costumam ser dele).")
            } else {
                format!("A porta {port} já está em uso por {who}, então {name} não consegue iniciar.")
            };
            let fix = if sys {
                "Não feche esse programa: mude a porta deste servidor em Configurações."
            } else {
                "Feche esse programa (aba Portas) ou mude a porta deste servidor em Configurações."
            };
            err(format!("{name}: a porta {port} está ocupada"), detail, fix)
        }
    }
}

fn servers(s: &Settings, listeners: &[ports::Listener], out: &mut Vec<Finding>) {
    let on: Vec<&ServiceCfg> = s.services.iter().filter(|c| c.enabled).collect();
    if on.is_empty() {
        out.push(warn("Nenhum servidor ligado", "Todos os servidores estão desligados em Configurações.", "Ligue ao menos o Apache e o MySQL."));
        return;
    }
    let reserved = excluded_ranges();
    for c in &on {
        let name = c.title();
        let d = services::def(&c.key);
        if services::version_dir(s, &c.id).is_none() {
            out.push(warn(
                format!("{name} não está instalado"),
                format!("Não achei {} em {}\\bin\\{}.", d.exe, s.root, d.dir),
                "Baixe pela janela \"Baixar e atualizar\" ou desligue este servidor.",
            ));
            continue;
        }
        out.push(port_finding(&name, c.port, &services::image(&c.key), listeners, &reserved));
    }
    // HTTPS needs Apache on 443 too.
    if s.https && on.iter().any(|c| c.key == "apache") {
        out.push(port_finding("Apache (HTTPS)", 443, &services::image("apache"), listeners, &reserved));
    }
    let pairs: Vec<(String, u16)> = on.iter().map(|c| (c.title(), c.port)).collect();
    for (port, names) in duplicate_ports(&pairs) {
        out.push(err(
            format!("Porta {port} repetida"),
            format!("{} estão na mesma porta; só um deles consegue iniciar.", names.join(" e ")),
            "Dê uma porta diferente a cada servidor em Configurações.",
        ));
    }
}

// ---------------------------------------------------------------- Apache

/// Reads the output of `httpd -t`: Ok, or the line that says what is wrong. "Syntax OK" wins over warnings
/// (the usual AH00558 about the server name does not stop Apache).
pub(crate) fn httpd_verdict(text: &str) -> Result<(), String> {
    if text.to_ascii_lowercase().contains("syntax ok") {
        return Ok(());
    }
    let lines: Vec<&str> = text.lines().map(str::trim).filter(|l| !l.is_empty()).collect();
    let pick = lines
        .iter()
        .find(|l| l.contains("AH0") || l.to_ascii_lowercase().contains("error") || l.to_ascii_lowercase().contains("cannot"))
        .or(lines.first())
        .map(|l| l.chars().take(300).collect::<String>());
    Err(pick.unwrap_or_else(|| "o Apache não respondeu".into()))
}

fn apache(s: &Settings, out: &mut Vec<Finding>) {
    if !s.svc("apache").enabled {
        return;
    }
    let Some(v) = services::version_dir(s, "apache") else { return };
    let exe = v.join(r"bin\httpd.exe");
    // conf/ninho.conf is written at the first start; before that httpd.conf still points at the zip's own folder
    // and the test would complain about folders that are fine.
    if !v.join(r"conf\ninho.conf").is_file() {
        out.push(ok("Apache: configuração ainda não preparada", "O Ninho prepara o httpd.conf no primeiro início; o teste de sintaxe roda depois disso."));
    } else {
        match run(&exe, &["-t"], Some(&v)) {
            None => out.push(err("Apache não abriu para o teste", format!("Não consegui executar {}.", exe.display()), "Veja se o Visual C++ está instalado e se o antivírus não bloqueou o httpd.exe.")),
            Some((_, text)) => match httpd_verdict(&text) {
                Ok(()) => out.push(ok("Configuração do Apache válida", "O teste \"httpd -t\" respondeu Syntax OK.")),
                Err(e) => out.push(err("Configuração do Apache com erro", e, "Abra o httpd.conf (botão Config do Apache), corrija a linha indicada e teste de novo.")),
            },
        }
    }
    // PHP as an Apache module.
    if let Some(php) = services::version_dir(s, "php") {
        if php_module(&php).is_some() {
            out.push(ok("Módulo PHP do Apache presente", "O PHP escolhido tem o módulo para rodar dentro do Apache."));
        } else {
            out.push(warn(
                "O PHP escolhido não tem módulo para o Apache",
                "Só as versões \"Thread Safe\" (TS) trazem o php*apache2_4.dll; sem ele os arquivos .php não são executados pelo Apache.",
                "Baixe um PHP Thread Safe (TS) ou use o servidor PHP embutido do Ninho.",
            ));
        }
    }
}

fn php_module(php: &Path) -> Option<PathBuf> {
    std::fs::read_dir(php).ok()?.filter_map(|e| e.ok()).map(|e| e.path()).find(|p| {
        let f = p.file_name().unwrap_or_default().to_string_lossy().to_ascii_lowercase();
        f.starts_with("php") && f.ends_with("apache2_4.dll")
    })
}

// ---------------------------------------------------------------- PHP

const KEY_EXTENSIONS: &[&str] = &["mysqli", "pdo_mysql", "mbstring", "openssl", "curl"];

/// The key extensions that are not switched on in php.ini (absent or commented out).
pub(crate) fn missing_exts(list: &[(String, bool)], wanted: &[&str]) -> Vec<String> {
    wanted.iter().filter(|w| !list.iter().any(|(n, on)| *on && n == *w)).map(|w| w.to_string()).collect()
}

/// The value of the active `extension_dir` line, without quotes.
pub(crate) fn extension_dir_value(ini: &str) -> Option<String> {
    ini.lines().map(str::trim).filter(|l| !l.starts_with(';')).find_map(|l| {
        let rest = l.strip_prefix("extension_dir")?.trim_start().strip_prefix('=')?;
        let v = rest.split(';').next().unwrap_or("").trim().trim_matches('"').to_string();
        (!v.is_empty()).then_some(v)
    })
}

fn php(s: &Settings, out: &mut Vec<Finding>) {
    let Some(v) = services::version_dir(s, "php") else { return };
    let label = v.file_name().unwrap_or_default().to_string_lossy().into_owned();
    // -n: no php.ini, so a broken ini cannot hide a broken PHP (and the other way round).
    match run(&v.join("php.exe"), &["-n", "-v"], Some(&v)) {
        Some((true, text)) if text.contains("PHP") => {
            out.push(ok(format!("PHP {label} executa"), text.lines().map(str::trim).find(|l| l.starts_with("PHP")).unwrap_or("php -v respondeu").to_string()));
        }
        _ => {
            out.push(err(
                format!("PHP {label} não executa"),
                "O php.exe não respondeu ao comando \"php -n -v\".",
                "Instale o Visual C++ 2015-2022 ou baixe o PHP de novo (o antivírus pode ter bloqueado o php.exe).",
            ));
            return;
        }
    }
    let ini = v.join("php.ini");
    let Ok(text) = std::fs::read_to_string(&ini) else {
        out.push(warn(
            "PHP sem php.ini",
            format!("Não achei {}. Sem ele o PHP roda com tudo desligado (sem mysqli, mbstring, curl...).", ini.display()),
            "Copie php.ini-development para php.ini na pasta do PHP.",
        ));
        return;
    };
    out.push(ok("php.ini encontrado", ini.display().to_string()));
    match extension_dir_value(&text) {
        None => out.push(warn("php.ini sem extension_dir", "Sem essa linha o PHP não acha as extensões.", "No php.ini, deixe a linha extension_dir = \"ext\" sem o ponto e vírgula.")),
        Some(d) => {
            let p = PathBuf::from(&d);
            let full = if p.is_absolute() { p } else { v.join(p) };
            if !full.is_dir() {
                out.push(warn("extension_dir aponta para uma pasta que não existe", format!("{} não foi encontrada.", full.display()), "Ajuste extension_dir no php.ini para a pasta ext do PHP."));
            }
        }
    }
    let bad = missing_exts(&phpini::list(&ini), KEY_EXTENSIONS);
    if bad.is_empty() {
        out.push(ok("Extensões principais do PHP ligadas", KEY_EXTENSIONS.join(", ")));
    } else {
        out.push(warn(
            "Extensões do PHP desligadas",
            format!("Desligadas ou ausentes no php.ini: {}. Muitos sistemas (WordPress, Laravel) precisam delas.", bad.join(", ")),
            "Ligue em \"Extensões do PHP\" ou tire o ponto e vírgula da linha extension= no php.ini.",
        ));
    }
}

// ---------------------------------------------------------------- MySQL

/// A short plain-language reason for a failed `SELECT 1`.
pub(crate) fn mysql_reason(e: &str) -> String {
    let low = e.to_ascii_lowercase();
    if low.contains("access denied") {
        "o root tem senha (o Ninho espera root sem senha)".into()
    } else if low.contains("can't connect") || low.contains("cant connect") || low.contains("2003") {
        "não consegui conectar na porta".into()
    } else {
        e.lines().find(|l| !l.trim().is_empty()).unwrap_or("erro desconhecido").trim().chars().take(200).collect()
    }
}

fn mysql(s: &Settings, listeners: &[ports::Listener], out: &mut Vec<Finding>) {
    for c in s.services.iter().filter(|c| c.enabled && c.key == "mysql") {
        let Some(v) = services::version_dir(s, &c.id) else { continue };
        let name = c.title();
        // Data folder: the first MySQL cannot start without it; an extra one is created at its first start.
        match services::mysql_data(s, c, &v) {
            Some(d) if Path::new(&d).is_dir() => out.push(ok(format!("{name}: pasta de dados encontrada"), d)),
            Some(d) if c.is_extra() => out.push(ok(format!("{name}: pasta de dados será criada no primeiro início"), d)),
            Some(d) => out.push(err(
                format!("{name}: a pasta de dados não existe"),
                format!("O my.ini aponta para {d}, que não foi encontrada.\nO MySQL não inicia sem ela."),
                "Corrija a linha datadir no my.ini ou recupere a pasta de dados.",
            )),
            None => out.push(warn(format!("{name}: my.ini sem datadir"), "Não achei a linha datadir no my.ini deste MySQL.", "Confira o my.ini (botão Config).")),
        }
        let running = port_status(ports::owner(listeners, c.port).map(|l| l.program.as_str()), &services::image("mysql")) == PortStatus::Own;
        if !running {
            continue;
        }
        match services::mysql_exec(s, &c.id, "SELECT 1", "") {
            Ok(()) => out.push(ok(format!("{name}: responde a consultas"), "Entrei como root, sem senha, e o SELECT 1 funcionou.")),
            Err(e) => out.push(warn(
                format!("{name} não aceitou o login do root"),
                format!("O servidor está de pé na porta {}, mas {}.", c.port, mysql_reason(&e)),
                "O criar-banco do Ninho usa root sem senha. Se você pôs senha, use o seu cliente SQL; senão veja o log do MySQL.",
            )),
        }
    }
}

// ---------------------------------------------------------------- sites and hosts

fn site_folder(s: &Settings, out: &mut Vec<Finding>) {
    if !Path::new(&s.doc_root).is_dir() {
        out.push(warn(
            "Pasta dos sites não existe",
            format!("{} não foi encontrada.", s.doc_root),
            "Crie a pasta ou escolha outra em Configurações.",
        ));
        return;
    }
    let n = sites::list(s).len();
    out.push(ok(format!("Pasta dos sites: {n} site(s)"), s.doc_root.clone()));
}

fn hosts_path() -> PathBuf {
    let win = std::env::var("SystemRoot").unwrap_or_else(|_| r"C:\Windows".into());
    Path::new(&win).join(r"System32\drivers\etc\hosts")
}

/// How many lines of the hosts file Ninho owns.
pub(crate) fn count_marked(hosts: &str) -> usize {
    hosts.lines().filter(|l| l.contains(sites::MARKER)).count()
}

fn hosts(s: &Settings, out: &mut Vec<Finding>) {
    let path = hosts_path();
    let Ok(text) = std::fs::read_to_string(&path) else {
        out.push(warn(
            "Não consegui ler o arquivo hosts",
            format!("{} não abriu para leitura; os endereços .test podem não funcionar.", path.display()),
            "Veja se o antivírus ou outro programa está bloqueando o arquivo.",
        ));
        return;
    };
    let marked = count_marked(&text);
    // Opening for append changes nothing; it only tells whether Windows would let us write without asking.
    let writable = std::fs::OpenOptions::new().append(true).open(&path).is_ok();
    let access = if writable { "O Ninho pode atualizá-lo direto." } else { "Para atualizá-lo o Ninho pede permissão de administrador (é normal)." };
    if !s.auto_vhosts {
        out.push(ok("Endereços automáticos desligados", format!("{marked} linha(s) do Ninho no hosts. {access}")));
        return;
    }
    let want = sites::list(s).len();
    if marked == want {
        out.push(ok(format!("Arquivo hosts em dia ({marked} endereço(s))"), access));
    } else {
        out.push(warn(
            "Arquivo hosts fora de sincronia",
            format!("Há {want} site(s) na pasta, mas {marked} linha(s) do Ninho no hosts. {access}"),
            "Abra a aba Sites e atualize; aceite o pedido de permissão do Windows.",
        ));
    }
}

fn https(s: &Settings, out: &mut Vec<Finding>) {
    if !s.https {
        return;
    }
    // Same file name certs.rs gives the local authority's certificate.
    let crt = certs::dir(s).join("ninho-ca.crt");
    if !crt.is_file() {
        out.push(warn("HTTPS ligado, mas sem certificado", "A autoridade local do Ninho ainda não foi criada.", "Atualize os sites (aba Sites) com o Apache instalado; ela é criada nessa hora."));
    } else if certs::ca_trusted(s) {
        out.push(ok("HTTPS: certificado local confiável", "O Windows confia na autoridade do Ninho."));
    } else {
        out.push(warn(
            "HTTPS: o Windows ainda não confia no certificado",
            "O navegador vai mostrar aviso de site inseguro nos endereços https.",
            "Use \"Confiar no certificado\" nas configurações de HTTPS.",
        ));
    }
}

// ---------------------------------------------------------------- disk

const GB: u64 = 1 << 30;

pub(crate) fn disk_level(free: u64) -> Level {
    if free < GB / 2 {
        Level::Error
    } else if free < 2 * GB {
        Level::Warn
    } else {
        Level::Ok
    }
}

#[cfg(windows)]
#[link(name = "kernel32")]
unsafe extern "system" {
    fn GetDiskFreeSpaceExW(dir: *const u16, free_to_caller: *mut u64, total: *mut u64, total_free: *mut u64) -> i32;
}

/// Free bytes on the drive that holds `path` (its nearest folder that exists, since the root may not be created yet).
#[cfg(windows)]
fn free_bytes(path: &Path) -> Option<u64> {
    use std::os::windows::ffi::OsStrExt;
    let dir = path.ancestors().find(|p| p.is_dir())?;
    let wide: Vec<u16> = dir.as_os_str().encode_wide().chain(std::iter::once(0)).collect();
    let (mut avail, mut total, mut free) = (0u64, 0u64, 0u64);
    // SAFETY: `wide` is NUL-terminated and the three outputs are valid u64 locals.
    let r = unsafe { GetDiskFreeSpaceExW(wide.as_ptr(), &mut avail, &mut total, &mut free) };
    (r != 0).then_some(avail)
}

#[cfg(not(windows))]
fn free_bytes(_path: &Path) -> Option<u64> {
    None
}

fn gb(b: u64) -> String {
    format!("{:.1}", b as f64 / GB as f64).replace('.', ",")
}

fn disk(s: &Settings, out: &mut Vec<Finding>) {
    let Some(free) = free_bytes(Path::new(&s.root)) else { return };
    let title = format!("Espaço livre no disco: {} GB", gb(free));
    match disk_level(free) {
        Level::Ok => out.push(ok(title, format!("Na unidade de {}.", s.root))),
        Level::Warn => out.push(warn(title, "Está ficando pouco; bancos de dados e logs crescem com o tempo.", "Libere espaço na unidade onde o Ninho está.")),
        Level::Error => out.push(err(title, "Quase sem espaço: o MySQL pode corromper dados e os downloads vão falhar.", "Libere espaço agora na unidade onde o Ninho está.")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn httpd_syntax_ok_even_with_warning() {
        let t = "AH00558: httpd.exe: Could not reliably determine the server's fully qualified domain name\r\nSyntax OK\r\n";
        assert!(httpd_verdict(t).is_ok());
    }

    #[test]
    fn httpd_error_picks_the_useful_line() {
        let t = "\nAH00526: Syntax error on line 12 of C:/x/conf/httpd.conf:\nInvalid command 'Foo'\n";
        let e = httpd_verdict(t).unwrap_err();
        assert!(e.starts_with("AH00526"), "{e}");
        assert!(httpd_verdict("").is_err());
    }

    #[test]
    fn port_owner_rules() {
        assert_eq!(port_status(None, "httpd.exe"), PortStatus::Free);
        assert_eq!(port_status(Some("HTTPD.exe"), "httpd.exe"), PortStatus::Own);
        assert_eq!(port_status(Some(""), "httpd.exe"), PortStatus::Own);
        assert_eq!(port_status(Some("skype.exe"), "httpd.exe"), PortStatus::Other);
    }

    #[test]
    fn excluded_ranges_parse() {
        let t = "\nProtocol tcp Port Exclusion Ranges\n\nStart Port    End Port\n----------    --------\n      1025        1124\n     50000       50059     *\n\n* - Administered port exclusions.\n";
        assert_eq!(parse_excluded(t), vec![(1025, 1124), (50000, 50059)]);
    }

    #[test]
    fn duplicates_found() {
        let l = vec![("Apache".to_string(), 80), ("Nginx".to_string(), 80), ("MySQL".to_string(), 3306)];
        let d = duplicate_ports(&l);
        assert_eq!(d.len(), 1);
        assert_eq!(d[0].0, 80);
        assert_eq!(d[0].1, vec!["Apache", "Nginx"]);
    }

    #[test]
    fn php_extensions_missing() {
        let l = vec![("mysqli".to_string(), true), ("curl".to_string(), false), ("mbstring".to_string(), true)];
        assert_eq!(missing_exts(&l, KEY_EXTENSIONS), vec!["pdo_mysql", "openssl", "curl"]);
    }

    #[test]
    fn extension_dir_read() {
        assert_eq!(extension_dir_value("; extension_dir = \"x\"\nextension_dir = \"ext\" ; comment\n").as_deref(), Some("ext"));
        assert_eq!(extension_dir_value(";extension_dir = \"ext\"\n"), None);
    }

    #[test]
    fn hosts_marker_count() {
        let t = format!("127.0.0.1 localhost\n127.0.0.1      a.test    {m}\n127.0.0.1      b.test    {m}\n", m = sites::MARKER);
        assert_eq!(count_marked(&t), 2);
    }

    #[test]
    fn disk_thresholds() {
        assert_eq!(disk_level(10 * GB), Level::Ok);
        assert_eq!(disk_level(2 * GB), Level::Ok);
        assert_eq!(disk_level(GB), Level::Warn);
        assert_eq!(disk_level(GB / 4), Level::Error);
        assert_eq!(gb(3 * GB / 2), "1,5");
    }

    #[test]
    fn mysql_reasons() {
        assert!(mysql_reason("ERROR 1045 (28000): Access denied for user 'root'").contains("senha"));
        assert!(mysql_reason("ERROR 2003 (HY000): Can't connect to MySQL server").contains("conectar"));
    }

    #[test]
    fn counts_levels() {
        let l = vec![ok("a", ""), warn("b", "", "x"), err("c", "", "y"), ok("d", "")];
        assert_eq!(counts(&l), (2, 1, 1));
    }

    #[test]
    fn a_panic_inside_a_check_is_contained() {
        let mut out = Vec::new();
        guarded("x", &mut out, |o| {
            o.push(ok("before", ""));
            panic!("boom");
        });
        assert_eq!(out.len(), 2);
        assert_eq!(out[1].level, Level::Warn);
    }
}
