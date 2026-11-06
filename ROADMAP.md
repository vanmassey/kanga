# Kanga 0.2.0 Roadmap

Kanga 0.2.0 is the next development milestone for the independent x86-64 operating-system project. It builds on the experimental boot, console, interrupt, physical-frame allocation and RAM-filesystem foundation established in 0.1.0.

This roadmap describes intended work, not features that are already available. Priorities may change as implementation and testing reveal architectural requirements. No release date is promised.

## Release goals

The central goal for 0.2.0 is to make the kernel's foundations safer and more extensible. Work should favour small, testable subsystems with clear ownership and interfaces over adding user-facing features before the underlying mechanisms are reliable.

### 1. Kernel reliability and diagnostics

- Expand automated tests for RTC decoding, keyboard scancode translation, KangaFS filename handling and allocator boundary conditions.
- Improve failure reporting for boot-time checks and fatal CPU exceptions.
- Keep serial diagnostics useful and stable for automated validation.
- Document subsystem invariants, unsafe code contracts and interrupt-context restrictions.
- Test error paths as well as successful operations.

**Completion criteria:** the relevant unit and QEMU tests cover normal operation and important failure cases; failures produce actionable diagnostics.

### 2. Interrupt handling and synchronisation

- Introduce a central interrupt/IRQ dispatch interface instead of embedding device-specific handling in the low-level entry path.
- Separate exception handling from hardware IRQ dispatch.
- Add correct handling for spurious legacy PIC interrupts.
- Define explicit rules for interrupt state, lock ordering and code that may run in interrupt context.
- Replace or supplement the current allocator spin lock with a design that cannot deadlock when an interrupt pre-empts a lock holder.
- Preserve the existing keyboard and PIT behaviour while migrating to the new interfaces.

**Completion criteria:** keyboard and timer interrupts use the common dispatch path; interrupt-context locking rules are documented and tested; the allocator cannot be deadlocked by the supported interrupt paths.

### 3. Memory management

- Introduce a page-table abstraction for inspecting and changing virtual-memory mappings.
- Add controlled page mapping and unmapping, with appropriate TLB invalidation.
- Improve physical-frame allocator tests, including exhaustion, invalid ranges, contiguous allocations and reclamation.
- Design a route to manage usable physical memory above the current 4 GiB bitmap limit.
- Implement a small kernel heap backed by physical frames and virtual mappings.
- Define allocation failure behaviour and ownership rules before converting existing subsystems to heap allocation.
- Handle bootloader-reclaimable memory only after the kernel has safely finished using the relevant boot information.

**Completion criteria:** mapping operations validate their inputs and maintain page-table invariants; heap allocation and deallocation pass boundary and stress tests; boot-time memory checks continue to pass.

### 4. Hardware discovery

- Add PCI configuration-space access and bus/device/function enumeration.
- Record discovered devices in a structured representation.
- Report vendor, device and class information over the serial console.
- Test enumeration in QEMU and make unsupported devices harmless.

**Completion criteria:** Kanga can enumerate and report PCI devices without relying on a particular device being present.

### 5. Storage and filesystem groundwork

- Define a block-device interface independent of a specific controller.
- Add a first storage driver suitable for the QEMU development environment.
- Add partition-table discovery if the selected device and test setup support it.
- Design a virtual filesystem interface so shell commands do not depend directly on KangaFS internals.
- Preserve KangaFS as a simple RAM-backed filesystem while the new interfaces are developed.
- Investigate an initial persistent filesystem only after block I/O and the VFS boundary are reliable.

**Completion criteria:** block-device operations have defined bounds and error handling; the RAM filesystem can be accessed through the filesystem interface; storage tests do not risk silently corrupting data.

Persistent storage is a major goal, but a complete on-disk filesystem is not guaranteed for 0.2.0. It should not be rushed to meet a version number.

## Not a 0.2.0 commitment

The following remain longer-term directions unless the core goals above are completed and the designs are reviewed:

- Multitasking, kernel threads and a process scheduler.
- User mode, system calls and an executable loader.
- Networking and network protocols.
- USB support and a broad range of physical hardware drivers.
- A graphical desktop or window system.
- SMP and multi-core scheduling.
- A general-purpose, feature-complete filesystem.

These features depend on reliable memory, interrupt, device and storage foundations. Their absence is not a failure of the 0.2.0 milestone.

## Testing and quality requirements

Every kernel change should preserve the project's freestanding Rust and no_std design unless a deliberate architectural decision is documented.

For relevant changes:

- Add unit tests for pure logic and boundary conditions.
- Extend QEMU tests for boot-time and hardware-facing behaviour.
- Keep serial output deterministic enough for CI to validate.
- Capture framebuffer output when a change affects the console.
- Test both success and failure paths.
- Document any behaviour that cannot be tested in QEMU and avoid claiming physical-hardware compatibility without evidence.

The existing build and validation workflow should remain useful throughout the milestone. A passing QEMU run is a baseline, not proof that Kanga works on every physical machine.

## Suggested implementation order

The order below reflects dependencies, not fixed deadlines:

1. Expand tests and document current invariants.
2. Introduce the interrupt dispatch design and make synchronisation safe for interrupt use.
3. Build page-table management and the kernel heap.
4. Add PCI enumeration.
5. Define block-device and VFS interfaces.
6. Add a QEMU storage driver and evaluate persistent filesystem support.

A task may be split into smaller issues and pull requests. Significant design changes should be discussed in an issue before implementation.

## Versioning and release policy

- **0.1.0** remains the first experimental milestone and its published release assets should remain unchanged.
- New development described here belongs to the **0.2.0** development line.
- The roadmap does not imply that any item is complete until its implementation, tests and documentation have been reviewed and merged.
- A 0.2.0 release should be made only when its completed scope is stable enough to describe accurately in the changelog and release notes.

## Contributing

Contributions are welcome in implementation, tests and documentation. Please review [CONTRIBUTING.md](CONTRIBUTING.md), check the [open issues](https://github.com/vanmassey/kanga/issues), and open an issue before starting a substantial architectural change.
