// Stub implementations for libc functions needed by std
// These are weak symbols that will be used only if not provided elsewhere

// Simple bump allocator for malloc
// Heap region: We'll use memory starting at 0x500000 (5MB mark)
#define HEAP_START 0x500000
#define HEAP_SIZE  0x100000  // 1MB heap

static unsigned long heap_ptr = HEAP_START;
static unsigned long heap_end = HEAP_START + HEAP_SIZE;

__attribute__((weak)) void* malloc(unsigned long size) {
    // Align to 16 bytes
    size = (size + 15) & ~15;
    
    unsigned long new_ptr = heap_ptr + size;
    if (new_ptr > heap_end) {
        // Out of memory
        return (void*)0;
    }
    
    void* result = (void*)heap_ptr;
    heap_ptr = new_ptr;
    return result;
}

__attribute__((weak)) void free(void* ptr) {
    // Bump allocator doesn't support free
    // This is acceptable for init process which doesn't free much
}

__attribute__((weak)) void* calloc(unsigned long nmemb, unsigned long size) {
    unsigned long total = nmemb * size;
    void* ptr = malloc(total);
    if (ptr) {
        // Zero the memory
        char* p = ptr;
        for (unsigned long i = 0; i < total; i++) {
            p[i] = 0;
        }
    }
    return ptr;
}

__attribute__((weak)) void* realloc(void* ptr, unsigned long size) {
    // Simple realloc: just allocate new memory and copy
    if (!ptr) return malloc(size);
    
    void* new_ptr = malloc(size);
    if (!new_ptr) return (void*)0;
    
    // We don't know the old size, so we can't copy properly
    // This is a limitation of bump allocator
    // For now, just return the new pointer
    return new_ptr;
}

// String functions
__attribute__((weak)) unsigned long strlen(const char* s) { 
    unsigned long len = 0;
    while (s[len]) len++;
    return len;
}
__attribute__((weak)) void* memcpy(void* dest, const void* src, unsigned long n) {
    char* d = dest;
    const char* s = src;
    for (unsigned long i = 0; i < n; i++) d[i] = s[i];
    return dest;
}
__attribute__((weak)) void* memset(void* s, int c, unsigned long n) {
    char* p = s;
    for (unsigned long i = 0; i < n; i++) p[i] = (char)c;
    return s;
}
__attribute__((weak)) void* memmove(void* dest, const void* src, unsigned long n) {
    char* d = dest;
    const char* s = src;
    if (d < s) {
        for (unsigned long i = 0; i < n; i++) d[i] = s[i];
    } else {
        for (unsigned long i = n; i > 0; i--) d[i-1] = s[i-1];
    }
    return dest;
}
__attribute__((weak)) int memcmp(const void* s1, const void* s2, unsigned long n) {
    const unsigned char* p1 = s1;
    const unsigned char* p2 = s2;
    for (unsigned long i = 0; i < n; i++) {
        if (p1[i] != p2[i]) return p1[i] - p2[i];
    }
    return 0;
}

// System call numbers (matching NexaOS kernel)
#define SYS_READ    0
#define SYS_WRITE   1
#define SYS_OPEN    2
#define SYS_CLOSE   3

// Inline assembly syscall wrapper
static inline long syscall3(long n, long a1, long a2, long a3) {
    long ret;
    __asm__ __volatile__(
        "int $0x81"
        : "=a"(ret)
        : "a"(n), "D"(a1), "S"(a2), "d"(a3)
        : "memory", "rcx", "r11"
    );
    return ret;
}

// I/O implementations using real syscalls
__attribute__((weak)) long read(int fd, void* buf, unsigned long count) {
    return syscall3(SYS_READ, fd, (long)buf, count);
}

__attribute__((weak)) long write(int fd, const void* buf, unsigned long count) {
    return syscall3(SYS_WRITE, fd, (long)buf, count);
}

__attribute__((weak)) int open(const char* pathname, int flags, ...) {
    return (int)syscall3(SYS_OPEN, (long)pathname, flags, 0);
}

__attribute__((weak)) int close(int fd) {
    return (int)syscall3(SYS_CLOSE, fd, 0, 0);
}

// Process control stubs
__attribute__((weak)) void exit(int status) { while(1); }
__attribute__((weak)) void _exit(int status) { while(1); }
__attribute__((weak)) int getpid(void) { return 1; }

// Environment stubs
__attribute__((weak)) char* getenv(const char* name) { return (char*)0; }
__attribute__((weak)) int setenv(const char* name, const char* value, int overwrite) { return -1; }
__attribute__((weak)) int unsetenv(const char* name) { return -1; }
__attribute__((weak)) char* getcwd(char* buf, unsigned long size) { return (char*)0; }

// Error handling
__attribute__((weak)) int* __errno_location(void) { 
    static int errno_value = 0;
    return &errno_value;
}

// Thread-local storage stubs
typedef struct {
    void (*destructor)(void*);
    void* data;
} pthread_key_data_t;

static pthread_key_data_t pthread_keys[128];
static int pthread_key_next = 0;

__attribute__((weak)) int pthread_key_create(unsigned int* key, void (*destructor)(void*)) {
    if (pthread_key_next >= 128) return -1;
    pthread_keys[pthread_key_next].destructor = destructor;
    pthread_keys[pthread_key_next].data = (void*)0;
    *key = pthread_key_next++;
    return 0;
}

__attribute__((weak)) int pthread_key_delete(unsigned int key) {
    if (key >= 128) return -1;
    pthread_keys[key].destructor = (void*)0;
    return 0;
}

__attribute__((weak)) void* pthread_getspecific(unsigned int key) {
    if (key >= 128) return (void*)0;
    return pthread_keys[key].data;
}

__attribute__((weak)) int pthread_setspecific(unsigned int key, const void* value) {
    if (key >= 128) return -1;
    pthread_keys[key].data = (void*)value;
    return 0;
}

// Unwind stubs (for panic handling)
struct _Unwind_Context;
typedef int _Unwind_Reason_Code;

__attribute__((weak)) unsigned long _Unwind_GetIP(struct _Unwind_Context* context) { 
    return 0; 
}

__attribute__((weak)) unsigned long _Unwind_GetIPInfo(struct _Unwind_Context* context, int* ip_before_insn) { 
    if (ip_before_insn) *ip_before_insn = 0;
    return 0; 
}

__attribute__((weak)) unsigned long _Unwind_GetCFA(struct _Unwind_Context* context) { 
    return 0; 
}

__attribute__((weak)) unsigned long _Unwind_GetGR(struct _Unwind_Context* context, int index) { 
    return 0; 
}

__attribute__((weak)) void _Unwind_SetGR(struct _Unwind_Context* context, int index, unsigned long value) {}

__attribute__((weak)) void _Unwind_SetIP(struct _Unwind_Context* context, unsigned long value) {}

__attribute__((weak)) unsigned long _Unwind_GetDataRelBase(struct _Unwind_Context* context) { 
    return 0; 
}

__attribute__((weak)) unsigned long _Unwind_GetTextRelBase(struct _Unwind_Context* context) { 
    return 0; 
}

__attribute__((weak)) unsigned long _Unwind_GetRegionStart(struct _Unwind_Context* context) { 
    return 0; 
}

__attribute__((weak)) unsigned long _Unwind_GetLanguageSpecificData(struct _Unwind_Context* context) { 
    return 0; 
}

typedef _Unwind_Reason_Code (*_Unwind_Trace_Fn)(struct _Unwind_Context*, void*);

__attribute__((weak)) _Unwind_Reason_Code _Unwind_Backtrace(_Unwind_Trace_Fn trace, void* trace_argument) { 
    return 0; 
}

__attribute__((weak)) _Unwind_Reason_Code _Unwind_RaiseException(void* exception_object) { 
    return 0; 
}

__attribute__((weak)) void _Unwind_Resume(void* exception_object) {}

__attribute__((weak)) void _Unwind_DeleteException(void* exception_object) {}

// dl stubs (dynamic linker)
__attribute__((weak)) int dladdr(const void* addr, void* info) { return 0; }
__attribute__((weak)) void* dlopen(const char* filename, int flags) { return (void*)0; }
__attribute__((weak)) void* dlsym(void* handle, const char* symbol) { return (void*)0; }
__attribute__((weak)) int dlclose(void* handle) { return 0; }
__attribute__((weak)) char* dlerror(void) { return (char*)0; }

// Memory mapping stubs
__attribute__((weak)) void* mmap(void* addr, unsigned long length, int prot, int flags, int fd, long offset) { 
    return (void*)-1; 
}
__attribute__((weak)) int munmap(void* addr, unsigned long length) { return -1; }
__attribute__((weak)) int mprotect(void* addr, unsigned long len, int prot) { return -1; }

// Signal handling stubs
struct sigaction;
__attribute__((weak)) int sigaction(int signum, const struct sigaction* act, struct sigaction* oldact) { return -1; }
__attribute__((weak)) int sigaltstack(const void* ss, void* old_ss) { return -1; }
__attribute__((weak)) int sigemptyset(void* set) { return 0; }
__attribute__((weak)) int sigaddset(void* set, int signum) { return 0; }

// Process/thread stubs
__attribute__((weak)) void abort(void) { while(1); }
__attribute__((weak)) int sched_yield(void) { return 0; }
__attribute__((weak)) int nanosleep(const void* req, void* rem) { return 0; }

// Memory alignment allocation
__attribute__((weak)) int posix_memalign(void** memptr, unsigned long alignment, unsigned long size) {
    // Simple implementation: just use malloc (ignore alignment for now)
    *memptr = malloc(size);
    return (*memptr == (void*)0) ? -1 : 0;
}

// Vector I/O
struct iovec { void* iov_base; unsigned long iov_len; };

__attribute__((weak)) long readv(int fd, const struct iovec* iov, int iovcnt) {
    // Implement readv using multiple read() calls
    long total = 0;
    for (int i = 0; i < iovcnt; i++) {
        long n = read(fd, iov[i].iov_base, iov[i].iov_len);
        if (n < 0) return total > 0 ? total : n;
        total += n;
        if (n < (long)iov[i].iov_len) break; // Short read
    }
    return total;
}

__attribute__((weak)) long writev(int fd, const struct iovec* iov, int iovcnt) {
    // Implement writev using multiple write() calls
    long total = 0;
    for (int i = 0; i < iovcnt; i++) {
        long n = write(fd, iov[i].iov_base, iov[i].iov_len);
        if (n < 0) return total > 0 ? total : n;
        total += n;
        if (n < (long)iov[i].iov_len) break; // Short write
    }
    return total;
}

// Syscall wrapper
__attribute__((weak)) long syscall(long number, ...) { return -1; }

// Auxiliary vector
__attribute__((weak)) unsigned long getauxval(unsigned long type) { return 0; }

// Process control
__attribute__((weak)) int pause(void) { return -1; }
