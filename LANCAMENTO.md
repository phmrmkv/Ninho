# Lançamento no Windows — checklist

O que falta para o Ninho estar impecável no Windows antes de publicar. ✅ feito e testado · 🟡 em andamento ·
⬜ a fazer · 🔑 depende do dono (conta, certificado, decisão).

## Confiança e instalação
- ✅ Atualização automática: o Ninho vê a release nova no GitHub, baixa o `.exe` certo (x64 ou x86), confere o SHA-256 e se troca sozinho (testes automáticos; o teste de verdade só depois da primeira release)
- 🔑 Assinatura digital do `.exe` (sem ela o Windows mostra "O Windows protegeu o computador"): SignPath (grátis para código aberto) ou Azure Trusted Signing; o passo no GitHub Actions fica pronto, falta a conta
- 🟡 Pacote no winget: manifestos da 0.6.2 com SHA-256 enviados ao winget-pkgs; esperando a revisão da Microsoft
- ✅ Pacote no Scoop: bucket em https://github.com/phmrmkv/scoop-ninho (`scoop bucket add ninho https://github.com/phmrmkv/scoop-ninho`)
- ✅ Portátil e instalável (Menu Iniciar, Aplicativos instalados, sem administrador)
- ✅ Build x64 e x86 no GitHub Actions, com SHA-256 de cada arquivo

## Primeira experiência
- ✅ Boas-vindas na primeira vez: idioma, pasta do Ninho e o que baixar agora (Apache, PHP e MySQL já marcados)
- 🟡 Importar de XAMPP, WampServer e Laragon: sites do `htdocs`/`www` e bancos de dados (feito, com testes; falta testar num PC que tenha XAMPP ou WampServer)
- ✅ Uma única cópia aberta: abrir o Ninho de novo traz a janela que já está na bandeja (testado)
- ✅ Idioma do Windows na primeira vez (18 idiomas)

## Robustez
- ✅ Registro de falhas: se o Ninho quebrar, grava o motivo em `tmp\ninho-crash.log` e mostra onde está
- ✅ Mensagens do registro de atividade traduzidas (18 idiomas; erros técnicos de programas externos ficam como vêm)
- ✅ Verificar o ambiente (Visual C++, portas, Apache, PHP, MySQL, hosts, HTTPS, disco)
- ✅ Testes automáticos (81) rodando no GitHub a cada envio

## Acessibilidade
- ✅ Leitor de tela (AccessKit, conferido pela automação de interface do Windows: cada botão diz o que faz e em qual servidor), anel de foco no teclado, Esc fecha a janela, Ctrl+1/2/3 trocam a aba
- ✅ Contraste conferido nos temas claro e escuro

## Projeto no GitHub
- ✅ CONTRIBUTING.md (como compilar, traduzir, enviar mudanças)
- ✅ SECURITY.md (como avisar uma falha de segurança)
- ✅ CODE_OF_CONDUCT.md
- ✅ Modelos de issue (erro, ideia, tradução) e de pull request
- ✅ README pensado para busca, FEATURES.md, ROADMAP.md, CHANGELOG.md, licença MIT ou GPL-3.0
- ✅ Página de apresentação em `docs/index.html` (capturas em inglês de uma pasta de exemplo, temas claro e escuro, celular); no ar em https://phmrmkv.github.io/Ninho/. O GIF ficou de fora (sem ffmpeg nesta máquina): as capturas trocam por abas
- ✅ Código no GitHub (phmrmkv/Ninho) e releases 0.6.0, 0.6.1 e 0.6.2 com x64, x86 e SHA256SUMS.txt
- ✅ Discussions, aviso privado de falhas e etiquetas ligados no GitHub
- ✅ Revisão de bugs do código novo: atualização só com SHA-256, importador numa cópia dos dados, reinício sem servidores órfãos
