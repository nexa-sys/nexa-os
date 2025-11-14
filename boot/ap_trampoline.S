.set TRAMPOLINE_BASE, 0x8000

.section .ap_trampoline,"ax",@progbits
.code16
.globl __ap_trampoline_start
.globl __ap_trampoline_end
.globl ap_gdt16_ptr
.globl ap_gdt64_ptr
.globl ap_pml4_ptr
.globl ap_stack_ptr
.globl ap_entry_ptr
.globl ap_arg_ptr

__ap_trampoline_start:
    cli
    cld
    mov %cs, %ax
    mov %ax, %ds
    mov %ax, %es
    xor %ax, %ax
    mov %ax, %ss
    mov $0x7000, %sp

    mov $TRAMPOLINE_BASE, %bx
    mov $(ap_gdt16_ptr - __ap_trampoline_start), %si
    add %bx, %si
    lgdt (%si)

    mov %cr0, %eax
    or $0x1, %eax
    mov %eax, %cr0

    .set PROT_OFFSET, protected_mode_entry - __ap_trampoline_start
    ljmp $0x08, $(TRAMPOLINE_BASE + PROT_OFFSET)

.code32
protected_mode_entry:
    mov $0x10, %ax
    mov %ax, %ds
    mov %ax, %es
    mov %ax, %ss
    mov %ax, %fs
    mov %ax, %gs

    mov $TRAMPOLINE_BASE, %ebx

    mov %cr4, %eax
    or $0x20, %eax
    mov %eax, %cr4

    mov $ap_pml4_ptr - __ap_trampoline_start, %ecx
    add %ebx, %ecx
    mov (%ecx), %eax
    mov %eax, %cr3

    mov $0xC0000080, %ecx
    rdmsr
    or $0x100, %eax
    wrmsr

    mov %cr0, %eax
    or $0x80000001, %eax
    mov %eax, %cr0

    mov $ap_gdt64_ptr - __ap_trampoline_start, %ecx
    add %ebx, %ecx
    lgdt (%ecx)

    .set LONG_OFFSET, long_mode_entry - __ap_trampoline_start
    ljmp $0x08, $(TRAMPOLINE_BASE + LONG_OFFSET)

.code64
long_mode_entry:
    mov $0x10, %ax
    mov %ax, %ds
    mov %ax, %es
    mov %ax, %ss
    mov %ax, %fs
    mov %ax, %gs

    movabs $TRAMPOLINE_BASE, %rax
    movabs $(ap_stack_ptr - __ap_trampoline_start), %rcx
    add %rcx, %rax
    mov (%rax), %rsp
    mov %rsp, %rbp

    movabs $TRAMPOLINE_BASE, %rax
    movabs $(ap_arg_ptr - __ap_trampoline_start), %rcx
    add %rcx, %rax
    mov (%rax), %rdi

    movabs $TRAMPOLINE_BASE, %rax
    movabs $(ap_entry_ptr - __ap_trampoline_start), %rcx
    add %rcx, %rax
    mov (%rax), %rax
    jmp *%rax

.align 8
ap_gdt16_ptr:
    .word 0
    .long 0

.align 8
ap_gdt64_ptr:
    .word 0
    .quad 0

.align 8
ap_pml4_ptr:
    .quad 0

.align 8
ap_stack_ptr:
    .quad 0

.align 8
ap_entry_ptr:
    .quad 0

.align 8
ap_arg_ptr:
    .quad 0

__ap_trampoline_end:
