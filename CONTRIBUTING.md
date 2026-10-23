# Contributing to LumaMap

Thanks for helping! LumaMap is projection mapping software for Linux, and the
most useful thing you can do is use it on a real wall and tell us how it went.

## Reporting a bug

Open an [issue](https://github.com/dio99/lumamap/issues/new/choose) and include:

- what you did, what you expected and what happened;
- your distribution, GPU and how you installed LumaMap (AppImage, Flatpak, source);
- the output of `lumamap --version` and, if it crashed, the terminal output
  of `RUST_LOG=info lumamap`;
- if you can, the `.lmap` project (☰ → **Collect project…** puts it and its
  media in one folder).

Photos or a short video of the projection help a lot.

## Suggesting a feature

Describe the show or installation you want to make, not only the button you
are missing – there may already be a simple way to do it. LumaMap's rule is
that everything stays easy to use, so ideas that remove steps are especially
welcome.

## Code

```bash
cargo run --release            # the app
cargo test --release           # all tests
cargo clippy --release --workspace
```

- See [ARCHITECTURE.md](ARCHITECTURE.md) for how the crates fit together.
- Every change to the project goes through a `Command`, so it can be undone.
- Every text in the user interface is written in Swedish and English:
  `t("svenska", "English")`.
- Code comments are in Swedish; English is fine in pull requests.
- Keep pull requests focused, and run the tests and clippy before you push.

By contributing you agree that your contribution is licensed under the
[AGPL-3.0-or-later](LICENSE), like the rest of LumaMap.
