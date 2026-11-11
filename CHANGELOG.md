# Changelog

## [1.2.0](https://github.com/creation-guidelines/tad-engine/compare/v1.1.0...v1.2.0) (2026-11-11)


### Features

* a status between two tables is drawn as a grid ([#20](https://github.com/creation-guidelines/tad-engine/issues/20)) ([81016b2](https://github.com/creation-guidelines/tad-engine/commit/81016b267e15fa5fc0c864379e51081a4a49603b))
* **oco:** tad's own backlog, milestones and session log - its path in the observatory ([#24](https://github.com/creation-guidelines/tad-engine/issues/24)) ([f7bc2b7](https://github.com/creation-guidelines/tad-engine/commit/f7bc2b7691255ee9b96d29a95aab27eec22190a2))
* **render:** an about table heads the index ([#19](https://github.com/creation-guidelines/tad-engine/issues/19)) ([f3f0aad](https://github.com/creation-guidelines/tad-engine/commit/f3f0aad081945f6b9aff4dc8c56b331eca528097))
* **render:** horizontal rule between log entries ([#13](https://github.com/creation-guidelines/tad-engine/issues/13)) ([273400f](https://github.com/creation-guidelines/tad-engine/commit/273400f9fff829bd80f4c87e2572bc32d85701b2))
* **render:** use definition lists instead of bold field labels ([#11](https://github.com/creation-guidelines/tad-engine/issues/11)) ([d879b22](https://github.com/creation-guidelines/tad-engine/commit/d879b22f625bfe94395c4f9c238710e9a28a47d1))
* **render:** write reStructuredText as well as Markdown ([#16](https://github.com/creation-guidelines/tad-engine/issues/16)) ([c9df587](https://github.com/creation-guidelines/tad-engine/commit/c9df587a5eb24452f1e1f63e8ab94b63f7ee1ce5))
* tad in Rust - tables declared as data, every row checked ([#33](https://github.com/creation-guidelines/tad-engine/issues/33)) ([9388a53](https://github.com/creation-guidelines/tad-engine/commit/9388a53ac1c1068f0ff43c21c6588332938fc3e3))
* **tad:** canon and set - canonical form and checked row edits, byte for byte as DuckDB writes ([#37](https://github.com/creation-guidelines/tad-engine/issues/37)) ([d1bd333](https://github.com/creation-guidelines/tad-engine/commit/d1bd33344dacfb2e7fed048a3d041a7b89bd99ed))
* **tad:** render - documents from data, Markdown and reStructuredText, byte for byte ([#38](https://github.com/creation-guidelines/tad-engine/issues/38)) ([8f2f0a0](https://github.com/creation-guidelines/tad-engine/commit/8f2f0a0dffd2cd9473ee987c1788be71d333427a))
* **tad:** rules across tables - unique ids, references, appears-in, no mirrored relations ([#35](https://github.com/creation-guidelines/tad-engine/issues/35)) ([0211fb2](https://github.com/creation-guidelines/tad-engine/commit/0211fb2a17e50d77415a86be153c609b72762b18))


### Bug Fixes

* **ci:** dependabot watches the engine's own requirements ([#17](https://github.com/creation-guidelines/tad-engine/issues/17)) ([672303f](https://github.com/creation-guidelines/tad-engine/commit/672303f78c0bb9235675915243affab29734a0bf))
* **ci:** namespace the dist tag as dist/vX.Y.Z, not vX.Y.Z ([#9](https://github.com/creation-guidelines/tad-engine/issues/9)) ([910ecb0](https://github.com/creation-guidelines/tad-engine/commit/910ecb0656d37899c827638438390568dacf8fb4))
* **tad:** a rule guarded by another field is a dependency, not a requirement ([#34](https://github.com/creation-guidelines/tad-engine/issues/34)) ([c53c2d3](https://github.com/creation-guidelines/tad-engine/commit/c53c2d3e9ee3f7b48350a853c571335c21f3ab32))

## [1.1.0](https://github.com/creation-guidelines/tad-engine/compare/v1.0.1...v1.1.0) (2026-09-22)


### Features

* **ci:** tag dist at each release, so consumers can pin a version ([#7](https://github.com/creation-guidelines/tad-engine/issues/7)) ([2c7030b](https://github.com/creation-guidelines/tad-engine/commit/2c7030ba6a3c498524a5078ddf976277f125e91d))

## [1.0.1](https://github.com/creation-guidelines/tad-engine/compare/v1.0.0...v1.0.1) (2026-09-22)


### Bug Fixes

* **dc:** detect BLOB columns and fail check() with a clear error ([#3](https://github.com/creation-guidelines/tad-engine/issues/3)) ([56325ae](https://github.com/creation-guidelines/tad-engine/commit/56325aedbb73c9af82bdcc91ad85e9dfa85c8afb))
* **repo:** distribute only engine/ via a dist branch, not the whole repo ([9f5f18b](https://github.com/creation-guidelines/tad-engine/commit/9f5f18b7e1749c42c38b03240fb05eec6b11587d))

## 1.0.0 (2026-09-22)


### Features

* initial tad-engine, extracted from text-as-data-template ([2916c4e](https://github.com/creation-guidelines/tad-engine/commit/2916c4eb492954c0530a7a6c333a4e40ebad53db))
