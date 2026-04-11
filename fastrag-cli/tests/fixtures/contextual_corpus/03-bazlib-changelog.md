# bazlib changelog

## 1.9.0

- Added a streaming JSON parser with backpressure support.
- Reworked error reporting so each diagnostic carries a source span.
- Dropped support for the legacy `BAZLIB_PROFILE` environment variable.

## 1.8.4

- Fixed a regression where empty arrays serialized as `null`.
- Improved performance of the deep-copy helper for nested maps.
