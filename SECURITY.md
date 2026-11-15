# Security policy

Security fixes target the latest published versions. Report vulnerabilities
privately through [GitHub private vulnerability reporting](https://github.com/talaagent/kdl-kit/security/advisories/new).
If that form is unavailable, email **github@igm.pub**. Please do not disclose
an unpatched vulnerability in a public issue.

Include the affected crate and version, enabled features, impact, and a
minimal reproduction. Treat KDL input as untrusted: parser crashes, stack
overflows, unbounded memory or time on crafted input, and panics in the
deserializer all count. Use synthetic documents and remove private
configuration values from reports.

Problems in the `kdl` crate itself should also be reported upstream at
[kdl-org/kdl-rs](https://github.com/kdl-org/kdl-rs).
