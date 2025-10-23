OUTPUT_FORMAT(elf64-x86-64)
OUTPUT_ARCH(i386:x86-64)
ENTRY(_start)

/* Force linker to include multiboot header */
EXTERN(multiboot_header_start)

SECTIONS
{
    BOOT_BASE = 0x00100000;
    . = BOOT_BASE;

    .boot.header ALIGN(8) : AT(ADDR(.boot.header))
    {
        KEEP(*(.boot.header))
    }

    .boot ALIGN(8) : AT(ADDR(.boot))
    {
        *(.boot.text)
    }

    .text ALIGN(4K) : AT(ADDR(.text))
    {
        *(.text*)
    }

    .rodata ALIGN(4K) : AT(ADDR(.rodata))
    {
        *(.rodata*)
    }

    .data ALIGN(4K) : AT(ADDR(.data))
    {
        *(.data*)
    }

    .bss ALIGN(4K) : AT(ADDR(.bss))
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
