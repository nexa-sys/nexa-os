OUTPUT_FORMAT(elf64-x86-64)
OUTPUT_ARCH(i386:x86-64)
ENTRY(_start)

/* Force linker to include multiboot header */
EXTERN(multiboot_header_start)
EXTERN(uefi_start)

PHDRS
{
    boot PT_LOAD FLAGS(5);  /* R E */
    rodata PT_LOAD FLAGS(4); /* R */
    data PT_LOAD FLAGS(6);   /* RW */
}

SECTIONS
{
    BOOT_BASE = 0x00100000;
    . = BOOT_BASE;

    .boot.header ALIGN(8) : AT(ADDR(.boot.header))
    {
        KEEP(*(.boot.header))
    } :boot

    .boot ALIGN(8) : AT(ADDR(.boot))
    {
        *(.boot.text)
    } :boot

    .ap_trampoline ALIGN(16) : AT(ADDR(.ap_trampoline))
    {
        *(.ap_trampoline*)
    } :boot

    .text ALIGN(4K) : AT(ADDR(.text))
    {
        *(.text*)
    } :boot

    .rodata ALIGN(4K) : AT(ADDR(.rodata))
    {
        *(.rodata*)
    } :rodata

    .data ALIGN(4K) : AT(ADDR(.data))
    {
        *(.data*)
        *(.initramfs_meta*)
        *(.initramfs_flag*)
        *(.gs_data*)
        *(.gs_data_pad*)
    } :data

    .kernel_cache ALIGN(4K) : AT(ADDR(.kernel_cache))
    {
        *(.kernel_cache*)
    } :data

    .bss ALIGN(4K) : AT(ADDR(.bss))
    {
        *(COMMON)
        *(.bss*)
    } :data

    .nexa.uefi_entry : {
        QUAD(ABSOLUTE(uefi_start))
    } :data

    /DISCARD/ :
    {
        *(.eh_frame)
        *(.comment)
    }
}