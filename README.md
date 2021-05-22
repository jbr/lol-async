# lol-async

This crate provides an awkward async interface for
[cloudflare/lol-html][lol-html]. It currently is built for
`futures`-flavored `AsyncRead`, but it would be straightforward to
adapt it for tokio-flavored AsyncRead.

To get started, see [the api docs][docs].

* [API Docs][docs] [![docs.rs docs][docs-badge]][docs]
* [Releases][releases] [![crates.io version][version-badge]][crate]
* [Contributing][contributing]
* [CI ![CI][ci-badge]][ci]
* [API docs for main][main-docs]

[ci]: https://github.com/jbr/lol-async/actions?query=workflow%3ACI
[ci-badge]: https://github.com/jbr/lol-async/workflows/CI/badge.svg
[releases]: https://github.com/jbr/lol-async/releases
[docs]: https://docs.rs/lol-async
[contributing]: https://github.com/jbr/lol-async/blob/main/.github/CONTRIBUTING.md
[crate]: https://crates.io/crates/lol-async
[docs-badge]: https://img.shields.io/badge/docs-latest-blue.svg?style=flat-square
[version-badge]: https://img.shields.io/crates/v/lol-async.svg?style=flat-square
[main-docs]: https://jbr.github.io/lol-async/lol_async/
[lol-html]: https://github.com/cloudflare/lol-html

## Safety
This crate uses `#![deny(unsafe_code)]`.

## License

<sup>
Licensed under either of <a href="LICENSE-APACHE">Apache License, Version
2.0</a> or <a href="LICENSE-MIT">MIT license</a> at your option.
</sup>

<br/>

<sub>
Unless you explicitly state otherwise, any contribution intentionally submitted
for inclusion in this crate by you, as defined in the Apache-2.0 license, shall
be dual licensed as above, without any additional terms or conditions.
</sub>
