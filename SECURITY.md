# Security policy

## Supported version

Security fixes are applied to the current `main` branch. This project has not yet declared a
stable release line with backported security support.

## Reporting a vulnerability

Do not disclose a suspected vulnerability, credential, certificate, token, server address, world
data, or provider identifier in a public issue or pull request.

Prefer GitHub private vulnerability reporting from the repository's **Security** tab. When that
path is unavailable, email `yuito.kiuchi.dev@gmail.com` with a minimal description and no live
credentials. Include reproduction details only after a private channel has been established.

A useful report identifies the affected commit or release, the trust boundary involved, the
expected behavior, and the observed behavior. Reports involving Akamai resources should use test
or already-deleted resource identifiers whenever possible.

## Secrets

The repository, CI workflows, issue tracker, and deterministic E2E tests must remain secret-free.
Live Akamai and Cloudflare API tokens, TLS private keys, and the restic password belong only on the
deployed control-plane host or in an explicitly protected deployment environment. Long-lived R2 S3
secret keys must not be installed on the control plane or remote nodes; nodes receive prefix-scoped
temporary credentials only after mTLS authentication.
