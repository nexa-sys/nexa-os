/* Version script for libnrlib.so
 * This forces _start and _start_c to be exported as GLOBAL symbols
 * in the dynamic symbol table for runtime lookup by ld-nrlib
 */
{
  global:
    _start;
    _start_c;
    __nexa_*;
    /* All public C library functions */
    *;
  local:
    /* Rust internal symbols */
    _ZN*;
};
