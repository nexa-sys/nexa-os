.set TRAMPOLINE_BASE, 0x8000

.section .ap_trampoline,"ax",@progbits
.code16
.globl __ap_trampoline_start
.globl __ap_trampoline_end
.globl ap_gdt16_ptr
.globl ap_gdt64_ptr
.globl ap_idt_ptr
.globl ap_pml4_ptr
.globl ap_stack_ptr
.globl ap_entry_ptr
.globl ap_arg_ptr

__ap_trampoline_start:
    cli
    cld
    # CRITICAL FIX: DS must be 0 for absolute addressing in real mode
    # CS is set to vector (0x08) by STARTUP IPI, but we need DS=0 to access trampoline data
    xor %ax, %ax
    mov %ax, %ds
    mov %ax, %es
    mov %ax, %ss
    mov $0x7000, %sp

    # Debug: 'R' for Real mode start (output AFTER segment setup)
    mov $0x3F8, %dx
    mov $'R', %al
    out %al, %dx

    # Load temporary GDT (must be in low memory for real mode access)
    mov $(TRAMPOLINE_BASE + temp_gdt16_ptr - __ap_trampoline_start), %si
    
    # Debug: 'g' before lgdt
    mov $'g', %al
    out %al, %dx
    
    lgdt (%si)
    
    # Debug: 'G' after lgdt
    mov $'G', %al
    out %al, %dx

    # Debug: 'c' before CR0
    mov $'c', %al
    out %al, %dx

    mov %cr0, %eax
    or $0x1, %eax
    mov %eax, %cr0

    # Debug: 'C' after CR0
    mov $'C', %al
    out %al, %dx

    # Debug: 'j' before ljmp
    mov $'j', %al
    out %al, %dx

    .set PROT_OFFSET, protected_mode_entry - __ap_trampoline_start
    ljmp $0x08, $(TRAMPOLINE_BASE + PROT_OFFSET)

.code32
protected_mode_entry:
    # Debug: 'P' for Protected mode
    mov $0x3F8, %dx
    mov $'P', %al
    out %al, %dx
    
    mov $0x10, %ax
    mov %ax, %ds
    mov %ax, %es
    mov %ax, %ss
    mov %ax, %fs
    mov %ax, %gs

    mov $TRAMPOLINE_BASE, %ebx

    # Debug: '4' for CR4 about to be set
    mov $0x3F8, %dx
    mov $'4', %al
    out %al, %dx

    # Set up CR4 for long mode and SSE support:
    # Bit 5 (PAE) = 1: Physical Address Extension (required for long mode)
    # Bit 9 (OSFXSR) = 1: Enable SSE/FXSAVE/FXRSTOR instructions
    # Bit 10 (OSXMMEXCPT) = 1: Enable unmasked SSE exceptions
    mov %cr4, %eax
    or $(0x20 | 0x200 | 0x400), %eax   # PAE | OSFXSR | OSXMMEXCPT
    mov %eax, %cr4

    # Debug: '3' for CR3 about to be set
    mov $0x3F8, %dx
    mov $'3', %al
    out %al, %dx

    mov $ap_pml4_ptr - __ap_trampoline_start, %ecx
    add %ebx, %ecx
    mov (%ecx), %eax
    mov %eax, %cr3

    # Debug: 'M' for MSR about to be set
    mov $0x3F8, %dx
    mov $'M', %al
    out %al, %dx

    # Enable Long Mode (LME, bit 8) and NX bit (NXE, bit 11) in EFER
    mov $0xC0000080, %ecx
    rdmsr
    or $0x900, %eax     # Set LME (0x100) and NXE (0x800)
    wrmsr

    # Debug: '0' for CR0 about to be set
    mov $0x3F8, %dx
    mov $'0', %al
    out %al, %dx

    mov %cr0, %eax
    or $0x80000001, %eax
    mov %eax, %cr0

    # Debug: 'G' for GDT about to be loaded
    mov $0x3F8, %dx
    mov $'G', %al
    out %al, %dx

    mov $ap_gdt64_ptr - __ap_trampoline_start, %ecx
    add %ebx, %ecx
    lgdt (%ecx)

    # Debug: 'F' for Far jump
    mov $0x3F8, %dx
    mov $'F', %al
    out %al, %dx

    .set LONG_OFFSET, long_mode_entry - __ap_trampoline_start
    ljmp $0x08, $(TRAMPOLINE_BASE + LONG_OFFSET)

.code64
long_mode_entry:
    # FIRST: Output a debug marker to serial port to confirm we reached long mode
    mov $0x3F8, %dx        # COM1 port
    mov $'L', %al          # 'L' for Long mode
    out %al, %dx
    
    mov $0x10, %ax
    mov %ax, %ds
    mov %ax, %es
    mov %ax, %ss
    mov %ax, %fs
    mov %ax, %gs

    # Debug: 'S' for Segments set
    mov $0x3F8, %dx
    mov $'S', %al
    out %al, %dx

    # CRITICAL: Load IDT before doing ANYTHING else
    # This prevents triple fault if any exception occurs
    movabs $TRAMPOLINE_BASE, %rax
    movabs $(ap_idt_ptr - __ap_trampoline_start), %rcx
    add %rcx, %rax
    lidt (%rax)
    
    # Debug: 'I' for IDT loaded
    mov $0x3F8, %dx
    mov $'I', %al
    out %al, %dx

    movabs $TRAMPOLINE_BASE, %rax
    movabs $(ap_stack_ptr - __ap_trampoline_start), %rcx
    add %rcx, %rax
    mov (%rax), %rsp
    mov %rsp, %rbp

    # Debug: 'T' for sTack set
    mov $0x3F8, %dx
    mov $'T', %al
    out %al, %dx

    movabs $TRAMPOLINE_BASE, %rax
    movabs $(ap_arg_ptr - __ap_trampoline_start), %rcx
    add %rcx, %rax
    mov (%rax), %rdi

    # Debug: 'A' for Args loaded
    mov $0x3F8, %dx
    mov $'A', %al
    out %al, %dx

    movabs $TRAMPOLINE_BASE, %rax
    movabs $(ap_entry_ptr - __ap_trampoline_start), %rcx
    add %rcx, %rax
    
    # Debug: output the address we're about to read from
    # rax now contains the address to read entry from
    mov %rax, %r9    # Save address in r9
    
    # Output 'P' then address in hex (all 8 nibbles of low 32 bits)
    mov $'P', %al
    out %al, %dx
    mov $'{', %al
    out %al, %dx
    
    # Output low 32 bits of r9 as 8 hex digits
    mov %r9d, %ecx
    mov $8, %r8d
.output_ptr_hex:
    rol $4, %ecx
    mov %ecx, %eax
    and $0xF, %eax
    cmp $10, %eax
    jb .ptr_is_digit
    add $('A' - 10), %al
    jmp .ptr_output_char
.ptr_is_digit:
    add $'0', %al
.ptr_output_char:
    out %al, %dx
    dec %r8d
    jnz .output_ptr_hex
    
    mov $'}', %al
    out %al, %dx
    
    # Now read from the address in r9
    mov (%r9), %rax
    
    # Save target address IMMEDIATELY before any register modifications
    mov %rax, %rbx
    
    # Debug: 'J' for Jump, then output target address in hex
    mov $0x3F8, %dx
    mov $'J', %al
    out %al, %dx
    
    # Output '[' then 8 hex digits of address then ']'
    mov $'[', %al
    out %al, %dx
    
    # Output high nibbles of address (bits 31-0, 8 hex digits)
    mov %ebx, %ecx
    mov $8, %r8d           # 8 hex digits
.output_hex_loop:
    rol $4, %ecx           # Rotate left to get next nibble
    mov %ecx, %eax
    and $0xF, %eax
    cmp $10, %eax
    jb .is_digit
    add $('A' - 10), %al
    jmp .output_char
.is_digit:
    add $'0', %al
.output_char:
    out %al, %dx
    dec %r8d
    jnz .output_hex_loop
    
    mov $']', %al
    out %al, %dx
    
    # Restore target address to r10 (we'll use this for the jump)
    mov %rbx, %r10
    
    # Final debug before jump - use r11 as temp to preserve r10
    mov $0x3F8, %dx
    mov $'!', %al
    out %al, %dx
    
    # Debug: Output RSP alignment BEFORE push (should be 0 = 16-byte aligned)
    mov %rsp, %rax
    and $0xF, %rax
    cmp $10, %rax
    jb .rsp_before_digit
    add $('A' - 10), %al
    jmp .rsp_before_out
.rsp_before_digit:
    add $'0', %al
.rsp_before_out:
    out %al, %dx
    
    # x86_64 ABI requires RSP to be misaligned by 8 bytes at function entry
    # (as if a CALL instruction had pushed a return address)
    # Push a dummy return address to satisfy this requirement
    push $0
    
    # Debug: Output RSP alignment AFTER push (should be 8)
    mov %rsp, %rax
    and $0xF, %rax
    cmp $10, %rax
    jb .rsp_after_digit
    add $('A' - 10), %al
    jmp .rsp_after_out
.rsp_after_digit:
    add $'0', %al
.rsp_after_out:
    out %al, %dx
    
    # Jump to the Rust AP entry point (use r10 which has the unmodified address)
    jmp *%r10
    
    # Should never reach here
    mov $'?', %al
    out %al, %dx
    hlt

.align 8
# Temporary GDT for real mode -> protected mode transition
# This must be in low memory (< 1MB) accessible in real mode
temp_gdt16:
    .quad 0x0000000000000000    # Null descriptor
    .quad 0x00cf9a000000ffff    # Code16 segment (base=0, limit=4GB, executable, readable)
    .quad 0x00cf92000000ffff    # Data16 segment (base=0, limit=4GB, writable)
temp_gdt16_end:

.align 8
temp_gdt16_ptr:
    .word temp_gdt16_end - temp_gdt16 - 1    # limit
    .long TRAMPOLINE_BASE + temp_gdt16 - __ap_trampoline_start  # base (physical address)

.align 8
ap_gdt16_ptr:
    .word 0
    .long 0

.align 8
ap_gdt64_ptr:
    .word 0
    .quad 0

.align 8
ap_idt_ptr:
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
