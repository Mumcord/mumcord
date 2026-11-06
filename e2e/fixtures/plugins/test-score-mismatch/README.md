# Fixture score-mismatch

A test plugin of the Mumcord E2E suite (see `e2e/README.md`). Its package declares a proprietary licence (`LicenseRef-Fixture`) while `mumcord-plugin serve-registry` always lists MIT, so the install prompt scores it higher than Browse did (AC-9.37).
Official packages must ship a README and a LICENSE, so the test plugins do too.
