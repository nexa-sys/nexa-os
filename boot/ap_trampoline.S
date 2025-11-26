.set TRAMPOLINE_BASE, 0x8000
# Maximum number of CPUs supported (must match acpi::MAX_CPUS = 1024)
.set MAX_CPUS, 1024
# Per-CPU data structure size: 3 * 8 bytes = 24 bytes (stack_ptr, entry_ptr, arg_ptr)
.set PER_CPU_DATA_SIZE, 24
# Per-CPU data array starts at a fixed offset after trampoline code
.set PER_CPU_DATA_OFFSET, 0x800   # At trampoline base + 2KB

.section .ap_trampoline,"ax",@progbits
.code16
.globl __ap_trampoline_start
.globl __ap_trampoline_end
.globl ap_gdt16_ptr
.globl ap_gdt64_ptr
.globl ap_idt_ptr
.globl ap_pml4_ptr
# Legacy single-AP pointers (kept for backward compatibility, used for fallback)
.globl ap_stack_ptr
.globl ap_entry_ptr
.globl ap_arg_ptr
# New per-CPU data array
.globl ap_per_cpu_data
# APIC ID to CPU index mapping table
.globl ap_apic_to_index

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

    # === Read APIC ID for per-CPU data lookup ===
    # APIC ID is at LAPIC base + 0x20, bits 24-31
    # Use xAPIC MMIO access (LAPIC base is usually 0xFEE00000)
    mov $0xFEE00020, %eax        # LAPIC ID register address
    mov (%eax), %eax             # Read LAPIC ID register
    shr $24, %eax                # APIC ID is in bits 24-31
    mov %eax, %esi               # Save APIC ID in ESI for later use
    
    # Debug: 'A' followed by hex digit of APIC ID low nibble
    mov $0x3F8, %dx
    mov $'A', %al
    out %al, %dx
    mov %esi, %eax
    and $0xF, %eax
    cmp $10, %eax
    jb .apic_digit
    add $('A' - 10), %al
    jmp .apic_out
.apic_digit:
    add $'0', %al
.apic_out:
    out %al, %dx

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

    # === PARALLEL INIT: Read APIC ID and lookup per-CPU data ===
    # ESI still contains APIC ID from 32-bit mode (preserved across mode switch)
    # But in 64-bit mode, we need to re-read it to be safe
    mov $0xFEE00020, %eax        # LAPIC ID register (32-bit address fits in eax)
    mov (%rax), %eax             # Read LAPIC ID register
    shr $24, %eax                # APIC ID is in bits 24-31
    mov %eax, %r12d              # Save APIC ID in r12 (callee-saved)
    
    # Debug: Output APIC ID
    mov $0x3F8, %dx
    mov $'#', %al
    out %al, %dx
    mov %r12d, %eax
    and $0xF, %eax
    cmp $10, %eax
    jb .apic_id_digit
    add $('A' - 10), %al
    jmp .apic_id_out
.apic_id_digit:
    add $'0', %al
.apic_id_out:
    out %al, %dx

    # === Look up CPU index from APIC ID using mapping table ===
    # ap_apic_to_index[apic_id] contains CPU index
    movabs $TRAMPOLINE_BASE, %rax
    movabs $(ap_apic_to_index - __ap_trampoline_start), %rcx
    add %rcx, %rax
    # r12d = APIC ID, use it as index (each entry is 1 byte)
    mov %r12d, %r13d            # Zero-extend r12d to r13 (mov clears upper bits)
    add %r13, %rax              # Address of ap_apic_to_index[apic_id]
    movzbl (%rax), %r13d        # r13 = CPU index (0-255)
    
    # Debug: Output CPU index
    mov $0x3F8, %dx
    mov $':', %al
    out %al, %dx
    mov %r13d, %eax
    and $0xF, %eax
    cmp $10, %eax
    jb .idx_digit
    add $('A' - 10), %al
    jmp .idx_out
.idx_digit:
    add $'0', %al
.idx_out:
    out %al, %dx

    # Check if CPU index is valid (0xFF means invalid)
    cmp $0xFF, %r13d
    jne .valid_cpu_index
    # Invalid CPU index - halt
    mov $'X', %al
    out %al, %dx
    hlt
    jmp . - 1

.valid_cpu_index:
    # === Calculate per-CPU data address ===
    # Per-CPU data is at: TRAMPOLINE_BASE + (ap_per_cpu_data - __ap_trampoline_start) + cpu_index * PER_CPU_DATA_SIZE
    # Structure: [stack_ptr:8][entry_ptr:8][arg_ptr:8] = 24 bytes per CPU
    movabs $TRAMPOLINE_BASE, %rax
    movabs $(ap_per_cpu_data - __ap_trampoline_start), %rcx
    add %rcx, %rax                   # rax = base of per-CPU data array
    mov %r13d, %ecx
    imul $PER_CPU_DATA_SIZE, %ecx   # ecx = cpu_index * 24
    add %rcx, %rax                   # rax = per-CPU data base address for this CPU
    mov %rax, %r14                   # Save per-CPU data address in r14
    
    # Debug: '>' before loading per-CPU data
    mov $0x3F8, %dx
    mov $'>', %al
    out %al, %dx

    # Load stack pointer from per-CPU data
    mov 0(%r14), %rsp               # stack_ptr is at offset 0
    mov %rsp, %rbp

    # Debug: 'T' for sTack set
    mov $'T', %al
    out %al, %dx

    # Load arg pointer from per-CPU data
    mov 16(%r14), %rdi              # arg_ptr is at offset 16

    # Debug: 'A' for Args loaded
    mov $'A', %al
    out %al, %dx

    # Load entry pointer from per-CPU data
    mov 8(%r14), %rax               # entry_ptr is at offset 8
    mov %rax, %r10                  # Save entry address in r10
    
    # Debug: output the entry address
    mov $'P', %al
    out %al, %dx
    mov $'{', %al
    out %al, %dx
    
    # Output low 32 bits of r10 as 8 hex digits
    mov %r10d, %ecx
    mov $8, %r8d
.output_entry_hex:
    rol $4, %ecx
    mov %ecx, %eax
    and $0xF, %eax
    cmp $10, %eax
    jb .entry_is_digit
    add $('A' - 10), %al
    jmp .entry_output_char
.entry_is_digit:
    add $'0', %al
.entry_output_char:
    out %al, %dx
    dec %r8d
    jnz .output_entry_hex
    
    mov $'}', %al
    out %al, %dx
    
    # Final debug before jump
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
    
    # Jump to the Rust AP entry point (use r10 which has the entry address)
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

# Legacy single-AP pointers (kept for backward compatibility)
.align 8
ap_stack_ptr:
    .quad 0

.align 8
ap_entry_ptr:
    .quad 0

.align 8
ap_arg_ptr:
    .quad 0

# === APIC ID to CPU index mapping table ===
# Supports up to 256 different APIC IDs
# Each entry is 1 byte: ap_apic_to_index[apic_id] = cpu_index (0xFF = invalid)
.align 8
ap_apic_to_index:
    .fill 256, 1, 0xFF          # Initialize all entries to invalid (0xFF)

# === Per-CPU data array ===
# Located after APIC mapping table
# Each CPU has 24 bytes: [stack_ptr:8][entry_ptr:8][arg_ptr:8]
# The offset is dynamically calculated: ap_per_cpu_data - __ap_trampoline_start
# Rust code MUST use this actual offset, not the hardcoded PER_CPU_DATA_OFFSET
# However, for simplicity, we pad to ensure alignment at 0x800 offset
.align 64
ap_per_cpu_data:
    .fill MAX_CPUS * PER_CPU_DATA_SIZE, 1, 0   # 1024 CPUs * 24 bytes = 24576 bytes

__ap_trampoline_end:
