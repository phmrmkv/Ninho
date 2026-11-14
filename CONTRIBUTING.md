# Contributing to Ninho

Thanks for wanting to help! Ninho is a small Rust app, and fixes, translations and ideas are all welcome.
Please read the [Code of Conduct](CODE_OF_CONDUCT.md) first; the short version is "be kind".

## Build and run

You need Windows and the stable [Rust toolchain](https://rustup.rs) (with the MSVC build tools).

```powershell
git clone https://github.com/paulohmaciel/Ninho.git
cd Ninho
cargo build --release
.\target\release\ninho.exe
```

For a quick loop, `cargo run` builds a debug version and starts it.

## Tests

```powershell
cargo test
```

CI runs the same command on every push and pull request, so please run it before you open one.

## Code style

- Run `cargo fmt` and keep `cargo clippy` quiet when you can.
- Comments are short and say **why**, not what. If the code already says what it does, skip the comment.
- Keep Ninho portable: one exe, settings in `ninho.json` beside it, no administrator rights, nothing written outside
  its own folder unless the user asked for it.
- User-facing text never goes in the code; it lives in `locales/` (see below).

## Add or translate a language

1. Copy `locales/en.json` to `locales/<code>.json` (for example `locales/sv.json`) and translate the values. Keep
   the keys and any placeholders such as `{name}` exactly as they are.
2. Add a line to `LOCALES` in `src/i18n.rs` with the code, the language's name written in that language, the
   `include_str!` of your file and its writing system.
3. Run `cargo test`. It checks that your file is valid JSON and has no key that `en.json` lacks (a missing key just shows in English).
4. Open a pull request. To fix a wrong word in an existing language, just edit its JSON file.

No time to do the whole file? Open a [translation issue](https://github.com/paulohmaciel/Ninho/issues/new/choose)
and say which language; a missing text falls back to English, so partial work is still useful.

## Propose a feature

Open a [feature request](https://github.com/paulohmaciel/Ninho/issues/new/choose) or start a thread in
[Discussions](https://github.com/paulohmaciel/Ninho/discussions) and tell us the problem you want solved, not only
the solution. Check [ROADMAP.md](ROADMAP.md) and [FEATURES.md](FEATURES.md) first, since it may already be planned.
For bigger changes, please talk before you code so nobody spends a weekend on something that does not fit.

## Commits and pull requests

- Branch from `main`, one topic per pull request, and keep it small enough to review.
- Write commit messages in the imperative ("Fix port check on 32-bit Windows"), with a line of context if it is not
  obvious.
- Say in the pull request what changed, why, and how you tried it. Screenshots help for anything visual.
- Add a line to `CHANGELOG.md` for changes people will notice.
- Make sure `cargo test` passes.

## License of contributions

Ninho is dual licensed: [MIT](LICENSE-MIT) **or** [GPL-3.0-only](LICENSE-GPL-3.0), at the user's choice. By sending a
contribution you agree that it is licensed under the same terms, MIT OR GPL-3.0-only, with no additional conditions.
