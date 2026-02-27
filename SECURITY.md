# Security Policy

## Supported Versions

| Version | Supported |
|---------|-----------|
| 0.1.x   | ✅ Yes    |

Older versions receive no security updates. Always use the latest release.

---

## Reporting a Vulnerability

**Please do not open a public GitHub issue for security vulnerabilities.**

To report a security issue privately:

1. Go to the [Security tab](https://github.com/swissprismia/pii-shield/security) of this repository.
2. Click **"Report a vulnerability"** to open a private advisory draft.
3. Fill in the details: description, steps to reproduce, potential impact, and (if available) a suggested fix.

Alternatively, email the maintainers directly (see the repository's contact info on the GitHub profile).

### What to expect

- **Acknowledgement**: Within 48 hours of receiving your report.
- **Initial assessment**: Within 5 business days — we will confirm whether the issue is valid and the severity.
- **Resolution**: We aim to release a patch within 30 days for critical issues, 90 days for lower severity.
- **Credit**: With your permission, we will credit you in the release notes.

We follow [responsible disclosure](https://en.wikipedia.org/wiki/Responsible_disclosure) and ask that you do the same — please do not disclose publicly until we have released a fix.

---

## Security Design Notes

PII Shield is designed with privacy as a core principle:

- **100% local processing** — no data ever leaves your machine. The Python sidecar runs entirely offline using local spaCy NLP models.
- **No telemetry** — the app does not phone home, collect analytics, or send any data to external services.
- **No PII stored to disk** — tokenized text and token mappings are held in memory only and cleared on app exit. The history log (if enabled) is also in-memory only.
- **Clipboard access is scoped** — the app only reads/writes the system clipboard for the purpose of PII detection and anonymization.
- **config.json** — the only file written to disk is the user's app configuration, which contains no PII (only a list of app keywords and settings).

---

## Dependency Security

We run automated dependency audits:

- **Rust**: `cargo audit` via GitHub Actions on every push and weekly schedule
- **Python**: `pip-audit` on `sidecar/requirements.txt` weekly
- **npm**: Dependabot monitors npm dependencies

If you discover a vulnerability in a dependency we use, please report it both to us (so we can update) and to the upstream project.
