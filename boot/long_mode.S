# Multiboot2 header  
.section .boot.header,"a",@progbits
.align 8

multiboot2_header_start:
    .long 0xe85250d6                # magic
    .long 0                         # architecture (i386)
    .long multiboot2_header_end - multiboot2_header_start
    .long -(0xe85250d6 + 0 + (multiboot2_header_end - multiboot2_header_start))
    
    # Information request tag (optional but recommended)
    .align 8
    .word 1     # type = information request
    .word 0     # flags
    .long 12    # size
    .long 6     # request memory map

    # End tag
    .align 8
    .word 0    # type
    .word 0    # flags
    .long 8    # size
multiboot2_header_end:

# Boot code
.section .boot.text,"ax",@progbits
.code32
.globl _start
.extern kmain

_start:
    cli

    # Save multiboot magic and info pointer FIRST
    movl %eax, multiboot_magic
    movl %ebx, multiboot_info

    # Initialize serial port 0x3F8
    movw $0x3F8, %dx
    
    # Disable interrupts
    addw $1, %dx          # 0x3F9: IER
    movb $0x00, %al
    outb %al, %dx
    
    # Enable DLAB
    addw $2, %dx          # 0x3FB: LCR
    movb $0x80, %al
    outb %al, %dx
    
    # Set divisor to 1 (115200 baud)
    movw $0x3F8, %dx      # 0x3F8: DLL
    movb $0x01, %al
    outb %al, %dx
    
    addw $1, %dx          # 0x3F9: DLH
    movb $0x00, %al
    outb %al, %dx
    
    # Disable DLAB, set 8N1
    addw $2, %dx          # 0x3FB: LCR
    movb $0x03, %al
    outb %al, %dx
    
    # Enable FIFO
    movw $0x3FA, %dx      # 0x3FA: FCR
    movb $0xC7, %al
    outb %al, %dx
    
    # Enable IRQs, set RTS/DTR
    movw $0x3FC, %dx      # 0x3FC: MCR
    movb $0x0B, %al
    outb %al, %dx
    
    # Write test character
    movw $0x3F8, %dx
    movb $'B', %al
    outb %al, %dx
    
    # Set up stack
    movl $stack_top, %esp
    movl %esp, %ebp
    
    # Debug: Stack set
    movw $0x3F8, %dx
    movb $'S', %al
    outb %al, %dx

    # Load GDT
    lgdt gdt64_pointer
    
    # Debug: GDT loaded
    movw $0x3F8, %dx
    movb $'G', %al
    outb %al, %dx

    movw $0x10, %ax
    movw %ax, %ds
    movw %ax, %es
    movw %ax, %fs
    movw %ax, %gs
    movw %ax, %ss
    
    # Debug: Segments loaded
    movw $0x3F8, %dx
    movb $'D', %al
    outb %al, %dx

    # Enable long mode and paging
    movl $pdpt_table, %eax
    orl $0x3, %eax
    movl %eax, pml4_table
    movl $0x0, pml4_table + 4

    movl $0x00000083, %eax       # map 0..1 GiB
    movl %eax, pdpt_table
    movl $0x0, pdpt_table + 4

    movl $0x40000083, %eax       # map 1..2 GiB
    movl %eax, pdpt_table + 8
    movl $0x0, pdpt_table + 12

    movl $0x80000083, %eax       # map 2..3 GiB
    movl %eax, pdpt_table + 16
    movl $0x0, pdpt_table + 20

    movl $0xC0000083, %eax       # map 3..4 GiB
    movl %eax, pdpt_table + 24
    movl $0x0, pdpt_table + 28
    
    # Debug: Page tables set
    movw $0x3F8, %dx
    movb $'P', %al
    outb %al, %dx

    movl $pml4_table, %eax
    movl %eax, %cr3

    movl %cr4, %eax
    orl $(1 << 5), %eax          # enable PAE
    movl %eax, %cr4
    
    # Debug: PAE enabled
    movw $0x3F8, %dx
    movb $'A', %al
    outb %al, %dx

    movl $0xC0000080, %ecx       # IA32_EFER
    rdmsr
    orl $(1 << 8), %eax          # enable LME
    wrmsr
    
    # Debug: Long mode enabled
    movw $0x3F8, %dx
    movb $'L', %al
    outb %al, %dx

    movl %cr0, %eax
    orl $(1 << 31), %eax         # enable paging
    orl $(1 << 0), %eax          # ensure protected mode
    movl %eax, %cr0
    
    # Debug: Paging enabled
    movw $0x3F8, %dx
    movb $'J', %al
    outb %al, %dx

    ljmp $0x08, $long_mode_entry

# -----------------------------------------------------------------------------
# Long mode entry point (64-bit).
# -----------------------------------------------------------------------------

.code64
long_mode_entry:
    # Debug: Entered 64-bit mode
    movw $0x3F8, %dx
    movb $'6', %al
    outb %al, %dx
    movb $'4', %al
    outb %al, %dx
    
    lea stack_top(%rip), %rsp

    # Load multiboot parameters
    movl multiboot_magic(%rip), %esi
    movq multiboot_info(%rip), %rdi
    
    # Debug: About to call kmain
    movw $0x3F8, %dx
    movb $'K', %al
    outb %al, %dx

    cld
    call kmain

1:
    hlt
    jmp 1b

.section .data,"aw",@progbits
.align 4
multiboot_magic:
    .long 0
.align 8
multiboot_info:
    .quad 0

.align 8
gdt64:
    .quad 0x0000000000000000    # Null descriptor
    .quad 0x00209A0000000000    # Code segment: 64-bit, executable, readable
    .quad 0x0000920000000000    # Data segment: writable
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
