# `zebra-crosslink`

<!-- Mark to add banner and badges -->

`zebra-crosslink` is [Shielded Labs](https://shieldedlabs.net)'s implementation of *Zcash
Crosslink*, a hybrid PoW/PoS consensus protocol for [Zcash](https://z.cash/).

Please refer to [The `zebra-crosslink` Book](https://shieldedlabs.github.io/zebra-crosslink), or you can view all of its source text in the `./book/src` directory in this repository.

## Specification work

Formal specification, model checking, and the dynamic-sigma controller live
under [`spec/`](spec/):

- [`spec/quint/`](spec/quint/) — Quint specs for the Crosslink baseline,
  resampling variant, dynamic-sigma controller, composed PoW + BFT models,
  and the inductive finality lemmas. See
  [`spec/quint/README.md`](spec/quint/README.md) for the file map and gate
  status.
- [`spec/dynamic-sigma-participation-marker.md`](spec/dynamic-sigma-participation-marker.md)
  — the production decision for the dynamic-sigma participation marker
  (the PoW header's `FatPointerToBftBlock`) and its four-check verifier
  contract.
- [`spec/roadmap-completion-2026-05-19.md`](spec/roadmap-completion-2026-05-19.md)
  — landed-work snapshot for the canonical roadmap.

## License

Zebra is distributed under the terms of both the MIT license and the Apache
License (Version 2.0). Some Zebra crates are distributed under the [MIT license
only](LICENSE-MIT), because some of their code was originally from MIT-licensed
projects. See each crate's directory for details.

See [LICENSE-APACHE](LICENSE-APACHE) and [LICENSE-MIT](LICENSE-MIT).
