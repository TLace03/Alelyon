# Contributing to Alelyon

Thank you for reading the code, and for any issue or change you send.

## How changes land

This repository is generated. Its files are exported from Alelyon's private source repository, which stays the
single source of truth, so a pull request here is not merged as is:

1. Open an issue, or a pull request with the change. Small, focused changes are easiest to take.
2. A maintainer reviews it. An accepted change is ported into the private source, with you credited
   (`Co-authored-by:`) in the commit.
3. The next export carries it back here, and the pull request is closed with a link to that export.

Files that are not exported (anything outside the export's list) are overwritten or removed by the next export, so
please do not rely on adding new top-level files here. A change to one of the modules is best sent to the module's
own repository ([Alelyon-Client](https://github.com/TLace03/Alelyon-Client) or
[Alelyon-OS](https://github.com/TLace03/Alelyon-OS)).

## What a change should carry

- The tests of what you changed passing, on Rust 1.97 or later, with the committed lockfile (`--locked`).
- No new dependency unless it is needed: each one is reviewed for licence, platforms and supply chain.
- A test for a fixed defect where one is practical.
- A new screenshot or animation only when it is taken from the open build, on a fresh profile, with demonstration
  data, and carries no metadata: the export refuses an image with text chunks, comments or EXIF.

## Licence of contributions

By contributing you agree that your contribution is licensed under the Apache License 2.0, as the rest of the
repository is (see section 5 of the [LICENSE](https://github.com/TLace03/Alelyon/blob/main/LICENSE)).

## Security issues

Not here: see [SECURITY.md](SECURITY.md).
