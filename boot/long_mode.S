.section .text
.code32
.globl _start
.extern kmain

_start:
    cli
    movl %eax, multiboot_magic
    movl %ebx, multiboot_info
    movl $stack_top, %esp
    movl %esp, %ebp

    call enable_long_mode
    call load_gdt

    ljmp $0x08, $long_mode_entry

# -----------------------------------------------------------------------------
# Enable long mode by constructing identity-mapped paging structures.
# -----------------------------------------------------------------------------

enable_long_mode:
    movl $pdpt_table, %eax
    orl $0x3, %eax               # present | writable
    movl %eax, pml4_table
    movl $0x0, pml4_table + 4

    movl $0x00000083, %eax       # present | writable | 1 GiB page
    movl %eax, pdpt_table
    movl $0x0, pdpt_table + 4

    movl $pml4_table, %eax
    movl %eax, %cr3

    movl %cr4, %eax
    orl $(1 << 5), %eax          # enable PAE
    movl %eax, %cr4

    movl $0xC0000080, %ecx       # IA32_EFER
    rdmsr
    orl $(1 << 8), %eax          # enable LME
    wrmsr

    movl %cr0, %eax
    orl $(1 << 31), %eax         # enable paging
    orl $(1 << 0), %eax          # ensure protected mode
    movl %eax, %cr0

    ret

# -----------------------------------------------------------------------------
# Load a 64-bit capable GDT (code selector = 0x08, data selector = 0x10).
# -----------------------------------------------------------------------------

load_gdt:
    lgdt gdt64_pointer

    movw $0x10, %ax
    movw %ax, %ds
    movw %ax, %es
    movw %ax, %fs
    movw %ax, %gs
    movw %ax, %ss

    ret

# -----------------------------------------------------------------------------
# Long mode entry point (64-bit).
# -----------------------------------------------------------------------------

.code64
long_mode_entry:
    lea stack_top(%rip), %rsp

    movl multiboot_magic(%rip), %esi
    movq multiboot_info(%rip), %rdi

    cld
    call kmain

1:
    hlt
    jmp 1b

.section .data,"aw",@progbits
.align 8
multiboot_magic:
    .quad 0
multiboot_info:
    .quad 0

.align 8
gdt64:
    .quad 0x0000000000000000
    .quad 0x00af9a000000ffff
    .quad 0x00af92000000ffff
gdt64_end:

.align 8
gdt64_pointer:
    .word gdt64_end - gdt64 - 1
    .quad gdt64

.section .bss,"aw",@nobits
.align 4096
pml4_table:
    .zero 4096

.align 4096
pdpt_table:
    .zero 4096

.align 16
stack_bottom:
    .zero 8192
stack_top:
