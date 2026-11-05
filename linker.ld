OUTPUT_FORMAT(elf64-x86-64)
OUTPUT_ARCH(i386:x86-64)
ENTRY(kanga_entry)

PHDRS
{
    requests PT_LOAD FLAGS(4);
    text PT_LOAD FLAGS(5);
    rodata PT_LOAD FLAGS(4);
    data PT_LOAD FLAGS(6);
}

SECTIONS
{
    . = 0xffffffff80000000;

    .limine_requests_start : ALIGN(8) {
        KEEP(*(.limine_requests_start))
    } :requests

    .limine_requests : ALIGN(8) {
        KEEP(*(.limine_requests))
    } :requests

    .limine_requests_end : ALIGN(8) {
        KEEP(*(.limine_requests_end))
    } :requests

    . = ALIGN(4K);
    .text : {
        *(.text .text.*)
    } :text

    . = ALIGN(4K);
    .rodata : {
        *(.rodata .rodata.*)
    } :rodata

    . = ALIGN(4K);
    .data : {
        *(.data .data.*)
    } :data

    .bss : ALIGN(4K) {
        *(COMMON)
        *(.bss .bss.*)
    } :data

    /DISCARD/ : {
        *(.eh_frame*)
        *(.note .note.*)
    }
}
