## Summary

<!-- What does this pull request change? -->

## Motivation

<!-- Why is this change needed? Link the related issue, if there is one. -->

Closes #

## Implementation

<!-- Briefly explain the approach and any architectural impact. -->

## Testing

- [ ] `cargo fmt --all -- --check`
- [ ] `python3 -m unittest discover -s tests -v`
- [ ] Built the ISO with `make iso`
- [ ] Booted and checked the result in QEMU
- [ ] Added or updated tests for changed behaviour

**Test environment:** <!-- QEMU version/configuration or physical hardware -->

## Documentation

- [ ] Updated relevant documentation, changelog entries and shell help text
- [ ] No documentation changes are needed

## Checklist

- [ ] The change is focused and does not include unrelated refactoring.
- [ ] Any necessary `unsafe` code includes a `// SAFETY:` explanation.
- [ ] I have considered interrupt-context and locking implications.
- [ ] I have not included secrets, credentials or unrelated generated files.
