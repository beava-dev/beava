# Security Policy

## Supported Versions

| Version | Supported |
|---------|-----------|
| 0.0.x   | yes       |

Only the latest release on `main` receives security fixes during pre-1.0.

## Reporting a Vulnerability

Email **hoang@beava.dev** with details. Do NOT open a public GitHub issue for
security vulnerabilities.

You can also file a private security advisory at
https://github.com/beava-dev/beava/security/advisories/new

Include in your report:

- A description of the vulnerability
- Steps to reproduce
- The affected commit hash (or release tag)
- Any potential impact

We aim to acknowledge reports within 72 hours and ship a fix within 14 days for
confirmed issues. Reporters are credited by name on request.

## Scope

Beava is designed to run behind a trusted boundary. By default:

- The TCP protocol has no authentication (bind to localhost or a private VPC in production).
- The HTTP admin sidecar binds a separate port; firewall it in production.
- There is no encryption in transit or at rest in v0 (terminate TLS at a reverse proxy).

Deployments exposing Beava directly to untrusted networks are out of scope for security
reports. Deployments following the documented configuration (localhost / private network /
reverse-proxy TLS termination) are in scope.

## Contact

Security: hoang@beava.dev
