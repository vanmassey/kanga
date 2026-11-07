# Kanga 0.2.0 and long-term development roadmap

Kanga is an independent x86-64 operating-system project. It is intended to grow into a capable, general-purpose operating system, not remain a tutorial kernel or a collection of isolated demonstrations.

This roadmap records the intended architectural direction. It does not claim that planned components already exist, and it does not promise a release date. All work described here belongs to the 0.2.0 development line until a later release plan is explicitly published. The published 0.1.0 release remains unchanged.

## Engineering direction

Kanga should be built as a coherent operating system with explicit subsystem boundaries, stable internal interfaces, resource ownership, meaningful failure handling and tests at multiple levels. Features should be substantial enough to support later features rather than temporary demonstrations that must be discarded.

The project should aim for:

- A maintainable monolithic kernel with clearly separated architecture-specific and architecture-independent code.
- Robust physical and virtual memory management, including ownership, reclamation, protection and accounting.
- Pre-emptive multitasking, isolated processes and a documented user/kernel ABI.
- A real device model, bus discovery, reusable driver interfaces and hardware-independent services.
- A virtual filesystem and persistent storage with defined on-disk formats and recovery/error behaviour.
- A layered network stack and a coherent userland.
- Security boundaries, capability/permission checks, defensive input validation and auditable unsafe code.
- Automated unit, integration, emulator and (when available) physical-hardware testing.

Do not add a feature merely to produce a boot message or a screenshot. A subsystem is not complete until its invariants, interfaces, error paths and tests are in place.

## Ordered implementation programme

The following order is deliberate and should be preserved. Work within a stage may be split into several focused pull requests. A later stage must not be started by pretending that an earlier stage's unfinished prerequisites are complete.

### 1. Kernel heap and complete memory foundations

Build a kernel memory subsystem that can support a growing kernel rather than a fixed demonstration allocator.

Scope:

- Replace the fixed-size, physically contiguous heap with a virtual-page-backed heap that can grow and shrink.
- Support aligned allocations, deallocation, splitting, coalescing, fragmentation management and allocation failure reporting.
- Establish a documented allocator API and ownership rules; do not expose raw allocator internals to every subsystem.
- Decide which allocation contexts are permitted. Interrupt handlers, exception paths and early boot must not accidentally invoke a sleeping or unsafe allocator.
- Add allocation accounting, heap statistics, corruption checks and diagnostic reporting.
- Expand physical-frame management beyond the current 4 GiB bitmap ceiling and provide efficient range allocation, reservation and reclamation.
- Complete virtual-memory management: independent address spaces, safe page-table creation/reclamation, mapping permissions, guard regions, TLB invalidation and architecture feature detection.
- Enable and validate NX before allowing non-executable page permissions; validate CPU paging capabilities at boot.
- Reclaim bootloader memory only after all bootloader-owned data has been copied or deliberately retained.
- Add stress, exhaustion, fragmentation, invalid-free and reuse tests.

Completion criteria: the heap is growable, backed by virtual mappings, has defined failure behaviour and passes allocator stress tests; memory ownership and page-table lifetime rules are documented; existing boot and QEMU tests remain green.

### 2. Multitasking and scheduling

Build a real tasking subsystem, not a cooperative task-switching demonstration.

Scope:

- Define task, thread, CPU context, kernel stack and lifecycle structures.
- Implement architecture-correct context switching with preserved callee-saved state, stack alignment and interrupt-state handling.
- Introduce a pre-emptive scheduler driven by timer interrupts, with explicit scheduling and interrupt-exit rules.
- Support task creation, blocking, waking, yielding, sleeping, termination and cleanup.
- Provide scheduler policies behind an interface; begin with a predictable priority-aware policy and leave room for fairness and load balancing.
- Separate per-CPU scheduler state from global task state in preparation for SMP.
- Define synchronisation primitives, wait queues, mutexes, condition variables and atomic interfaces with clear interrupt-context constraints.
- Ensure a task cannot free its active stack or address space before switching away safely.
- Add scheduler tracing, task identifiers, state inspection and stress tests.

Completion criteria: multiple kernel threads execute pre-emptively; blocking and wake-up work correctly; task resources are reclaimed safely; scheduler tests cover races and state transitions.

### 3. User-mode execution and process isolation

Introduce a real privilege boundary between kernel and applications.

Scope:

- Configure ring 3 execution, TSS privilege stacks and safe entry/exit paths.
- Create independent process address spaces with user/kernel mapping separation.
- Enforce user, supervisor, writable and non-executable page permissions.
- Provide guarded user and kernel stacks, controlled address-space switching and safe process teardown.
- Define process and thread ownership, parent/child relationships, exit status and wait semantics.
- Load a documented executable format, initially ELF64, with strict bounds and segment validation.
- Establish an initial userland image and a reproducible way to build and package programs.
- Prevent user pointers from being dereferenced directly by kernel code; add checked copy-in/copy-out helpers.
- Test invalid pointers, malformed executables, privilege transitions and isolation boundaries.

Completion criteria: an unprivileged process can be loaded and run independently; faults and termination are contained; user code cannot access kernel memory through ordinary mappings.

### 4. System-call interface and user/kernel ABI

Design a stable, versioned boundary between user programs and the kernel.

Scope:

- Define the syscall calling convention, register use, numbering, return/error convention and ABI versioning.
- Implement a safe x86-64 syscall entry/exit path, including stack switching and validation of user execution state.
- Provide checked user-memory access and consistent error handling.
- Add process, thread, memory, time, file, directory and device operations as their supporting subsystems become available.
- Define file descriptors/handles, per-process tables, inheritance and close-on-exec behaviour.
- Add permission checks and a clear policy for privileged operations.
- Keep architecture-specific syscall entry separate from portable syscall dispatch.
- Provide userland headers, libraries and tests so applications do not depend on kernel internals.
- Document ABI compatibility expectations before treating the interface as stable.

Completion criteria: user applications can invoke a documented syscall set; invalid numbers, arguments and pointers fail safely; ABI tests cover both success and error cases.

### 5. Persistent disk storage and filesystem services

Turn the existing RAM-only filesystem into a storage architecture capable of retaining data across reboot.

Scope:

- Implement a device model and PCI enumeration sufficient to discover storage controllers.
- Define a block-device interface with bounded reads/writes, flush semantics, capacity reporting, timeouts and error codes.
- Add a QEMU-supported storage driver first, then broaden controller support without coupling filesystems to a particular device.
- Discover partitions and validate partition-table bounds and metadata.
- Build a VFS with mount points, directory traversal, file handles, metadata, permissions and filesystem-specific operations.
- Retain KangaFS as a RAM filesystem and use it to test the VFS independently of disk hardware.
- Select and implement a first persistent filesystem with documented on-disk structures, consistency rules and recovery behaviour.
- Add caching and buffering only with explicit invalidation and flush rules.
- Protect against malformed disk images, truncated reads, write failures and out-of-range requests.
- Build host-side image tools and reproducible disk-image tests.

Completion criteria: files can be created, read, changed and removed on a virtual disk and remain present after reboot; filesystem errors do not silently become successful writes; RAM and persistent filesystems share the VFS interface.

### 6. Multi-core processing and interrupt controllers

Move from a single boot CPU to a properly coordinated SMP kernel.

Scope:

- Discover processor topology and initialise application processors through the platform firmware/boot protocol.
- Implement local APIC setup, I/O APIC routing, interrupt-source overrides and spurious interrupt handling.
- Define per-CPU data, stacks, scheduler state, interrupt state and CPU-local storage.
- Add inter-processor interrupts for rescheduling, TLB shootdowns and coordination.
- Make locks and memory reclamation correct across CPUs; define lock ordering and avoid unbounded interrupt-disabled critical sections.
- Add per-CPU allocators/caches where justified by measurement.
- Extend scheduling to multiple CPUs, with affinity and load balancing.
- Handle CPU startup failure and processor hotplug only when the underlying platform support is understood.
- Test concurrent allocation, address-space changes, scheduling and IPI delivery under QEMU SMP.

Completion criteria: multiple CPUs execute kernel work concurrently; interrupt routing and TLB shootdowns are correct; SMP stress tests do not expose races or deadlocks.

### 7. Networking

Build a layered network stack with clean driver and protocol boundaries.

Scope:

- Define network-device and packet-buffer interfaces.
- Add a virtualised Ethernet driver for the primary QEMU test environment.
- Implement Ethernet framing, ARP, IPv4, ICMP, UDP and TCP in independently testable layers.
- Add address configuration, routing tables, neighbour resolution, socket buffers and timeout handling.
- Define a socket API for user applications after the syscall and process foundations exist.
- Add DNS resolver support and basic network configuration tools.
- Validate packet lengths, checksums, fragmentation policy, state transitions and malformed input.
- Add host-side protocol tests and QEMU integration tests using an isolated virtual network.
- Design for later IPv6 support rather than embedding IPv4 assumptions in generic interfaces.

Completion criteria: userland programs can communicate over a QEMU virtual network using documented socket operations; malformed packets are handled safely; protocol tests cover normal and adversarial cases.

### 8. USB device support

Implement USB as a subsystem, not a one-off keyboard driver.

Scope:

- Add USB host-controller discovery and a controller abstraction.
- Implement xHCI as the initial modern host-controller target, with explicit DMA and memory-coherency requirements.
- Manage device enumeration, descriptors, configuration, interfaces, endpoints and transfer lifecycles.
- Implement transfer rings, event handling, cancellation, timeouts and device removal safely.
- Add hub support and a class-driver framework.
- Begin with HID keyboards and mice, then mass-storage support and other useful device classes.
- Ensure USB storage integrates through the block-device interface and USB HID integrates through the input subsystem.
- Test virtual USB devices in QEMU and document physical hardware coverage separately.

Completion criteria: Kanga can enumerate and operate supported USB devices through reusable class and controller interfaces; disconnects and failed transfers do not corrupt kernel state.

## Cross-cutting work

These are continuous requirements, not a separate final feature stage:

### Architecture and drivers
- Keep x86-64 mechanisms behind architecture interfaces where practical.
- Maintain a device model and reusable driver lifecycle.
- Use explicit DMA mapping and cache-coherency rules.
- Keep hardware discovery separate from driver policy.

### Security and reliability
- Treat all device, disk, network and user input as untrusted.
- Validate sizes, offsets, pointers, alignment and integer arithmetic at subsystem boundaries.
- Keep unsafe Rust blocks small and document their invariants.
- Add privilege separation, least privilege and permission checks as the relevant facilities arrive.
- Make panic, exception and device-failure paths diagnosable without depending on heap allocation.

### Userland and system experience
- Build a coherent base userland: init process, shell, core utilities, configuration tools and service management.
- Keep the kernel independent of shell command implementations.
- Define a package and application format only after executable loading, filesystems and process APIs are stable.
- Consider a graphical stack only after input, memory, scheduling, storage and userland foundations are mature.

### Testing and release quality
- Run formatting, static checks, unit tests, kernel builds and QEMU integration tests for every relevant change.
- Add property-based or fuzz testing for parsers, filesystems, protocol handlers and user-facing syscall inputs where practical.
- Preserve serial logs and machine-readable test checkpoints.
- Test multiple memory sizes, CPU counts and device configurations in emulation.
- Maintain a hardware compatibility matrix; QEMU success is not evidence of physical-hardware support.
- Keep documentation and changelogs aligned with actual merged functionality.

## Current milestone and release policy

- **0.1.0** is the published experimental foundation. Its release assets remain unchanged.
- **0.2.0** is the active development line. The eight stages above are the ordered architectural programme; work may span multiple releases if quality requires it.
- A stage is complete only when its implementation, tests, documentation and failure behaviour have been reviewed.
- Do not claim a subsystem is supported merely because a minimal path works in one emulator configuration.
- Do not merge unfinished or failing work to meet a schedule. There is no fixed release date.

## Contributing

Contributions in implementation, testing, architecture review and documentation are welcome. Read [CONTRIBUTING.md](CONTRIBUTING.md), review the [open issues](https://github.com/vanmassey/kanga/issues), and discuss substantial architectural changes before implementation.
