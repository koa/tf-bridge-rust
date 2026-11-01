## Commits

- One focused commit per logical change; split unrelated work (e.g. a refactor and the feature built on it) into separate commits.
- Subject: short, lowercase, imperative, no trailing period (e.g. `print panel labels`).
- Body: why the change was needed and anything non-obvious, wrapped at ~72 columns.
- Update CLAUDE.md in the same commit as the change it describes.
- Commits by Claude end with the `Co-Authored-By: Claude …` trailer, without a session link.
- Pushing to `master` publishes the image and chart (CI), so only push when asked.