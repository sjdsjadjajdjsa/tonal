# The Mac that builds releases

GitLab's free plan has no hosted Macs, so the `macos` job in `.gitlab-ci.yml` runs on
a Mac of our own: an Apple Silicon Mac with Rust (rustup) and Xcode's command line
tools, signed in, running GitLab Runner with the shell executor and the `tonal-macos`
tag. It only takes release jobs, so it can be off between releases; a release waits
for it.

## Set it up

1. `brew install gitlab-runner`
2. In GitLab, Settings > CI/CD > Runners > New project runner: tag `tonal-macos`, leave
   "Run untagged jobs" off, create it and copy the `glrt-` token.
3. `gitlab-runner register --non-interactive --url https://gitlab.com --token <glrt-token> --executor shell --name tonal-mac`
4. `brew services start gitlab-runner`

The runner then shows as online in Settings > CI/CD > Runners.

## Take it off

`brew services stop gitlab-runner`, delete the runner in Settings > CI/CD > Runners,
then `brew uninstall gitlab-runner`. Builds it left are in `~/builds`.
