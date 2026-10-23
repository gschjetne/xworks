# Changelog

All notable changes to this project are written down here. The format
follows [Keep a Changelog][kac], and versions follow [Semantic
Versioning][semver].

## [Unreleased]

### Added

+ A `--quiet` flag that prints only errors.
+ Plans can be printed in Norwegian and German.

### Fixed

+ Leap years no longer shift the plan by a day ([#31]).

## [1.2.0] - 2026-09-30

### Changed

+ The default frost date is now the median for the latitude, not the
  earliest recorded one. Plans for northern gardens start about a week
  later.
+ `larkspur plan` sorts crops by sowing date, then by name:
  + carrots before chard,
  + chard before kale.

### Removed

+ The `--legacy` output format, deprecated since 1.0.

## [1.1.0] - 2026-05-02

### Added

+ Beds can have a `shade` setting.

[kac]: https://keepachangelog.com/en/1.1.0/
[semver]: https://semver.org/
[#31]: https://example.org/larkspur/issues/31
[Unreleased]: https://example.org/larkspur/compare/v1.2.0...HEAD
[1.2.0]: https://example.org/larkspur/compare/v1.1.0...v1.2.0
[1.1.0]: https://example.org/larkspur/compare/v1.0.0...v1.1.0
