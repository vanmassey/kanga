# Security Policy

## Reporting a vulnerability

Please do not report security vulnerabilities in a public GitHub issue, discussion, pull request or commit.

If you believe you have found a security vulnerability in Kanga, please report it privately through GitHub:

1. Open the [Kanga repository](https://github.com/vanmassey/kanga).
2. Select the **Security** tab.
3. If **Report a vulnerability** is available, use it to submit a private vulnerability report.
4. Include the details listed below.

If private vulnerability reporting is not available, contact the repository maintainer privately through a contact method listed on their GitHub profile. Do not post exploit details publicly while seeking a private contact route.

## What to include

A useful report should include, where possible:

- A concise description of the suspected vulnerability and its potential impact.
- The affected version, release, commit or component.
- The architecture and environment used (for example, QEMU version and configuration, or physical hardware).
- Clear reproduction steps, including relevant build options and configuration.
- A proof of concept or minimal test case, if one can be provided safely.
- Relevant serial output, logs, screenshots or crash information.
- Any suggested mitigation or fix, if known.

Please distinguish confirmed behaviour from assumptions. Reports that are incomplete are still welcome; provide what you have.

## Responsible disclosure

Please allow the maintainer reasonable time to investigate and prepare a fix before disclosing the vulnerability publicly. Coordinate publication with the maintainer where possible.

Please do not:

- Publish exploit code or detailed reproduction steps before a fix or an agreed disclosure date.
- Use a vulnerability to access, modify or destroy data that is not yours.
- Test against systems or devices without permission.
- Disrupt services, damage hardware or perform destructive testing.
- Attempt to expose private information or credentials.

Security research should be limited to systems you own or are explicitly authorised to test. Kanga is an experimental operating-system project; testing should use an isolated virtual machine wherever practical.

## Scope

This policy covers security issues in the Kanga project itself, including:

- Kernel memory management and memory safety.
- CPU exception and hardware interrupt handling.
- Boot and image-loading paths maintained by this repository.
- Filesystem handling and storage code.
- Device drivers and hardware-facing interfaces.
- Build and release workflows where they could compromise project artefacts.

Issues in third-party tools or dependencies should be reported to their respective maintainers as well. If a Kanga integration introduces or amplifies the issue, please report that impact to this project too.

## Supported versions

Security fixes are generally expected to target the current development branch. The 0.1.0 release is an experimental milestone; users should not assume it receives ongoing security maintenance. The maintainer will state explicitly which releases receive fixes if a supported-release policy is introduced.

## Response and disclosure

Kanga is an independent project and does not currently promise a fixed response or remediation timeframe. The maintainer will make a reasonable effort to acknowledge reports, assess their impact, keep the reporter informed where practical and coordinate responsible disclosure.

Please do not assume that a report has been received until you receive an acknowledgement.

## No security guarantee

Kanga is experimental software, supplied under the terms of its licence and without a guarantee that it is secure or suitable for production use. Do not run it on systems or with data that require a level of security or reliability it has not demonstrated.
