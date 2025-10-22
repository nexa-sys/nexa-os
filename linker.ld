OUTPUT_FORMAT(elf64-x86-64)
OUTPUT_ARCH(i386:x86-64)
ENTRY(_start)

SECTIONS
{
    BOOT_BASE = 0x00400000;
    . = BOOT_BASE;

    .boot ALIGN(8) : AT(ADDR(.boot) - BOOT_BASE)
    {
        boot_header_start = .;
        KEEP(*(.boot.header))
        boot_header_end = .;
    }

    .text ALIGN(4K) : AT(ADDR(.text) - BOOT_BASE)
    {
        *(.text*)
    }

    .rodata ALIGN(4K) : AT(ADDR(.rodata) - BOOT_BASE)
    {
        *(.rodata*)
    }

    .data ALIGN(4K) : AT(ADDR(.data) - BOOT_BASE)
    {
        *(.data*)
    }

    .bss ALIGN(4K) : AT(ADDR(.bss) - BOOT_BASE)
    {
        *(COMMON)
        *(.bss*)
    }

    /DISCARD/ :
    {
        *(.eh_frame)
        *(.comment)
    }
}
