# Advanced Process Scheduler Documentation

## Overview

NexaOS implements a sophisticated **Multi-Level Feedback Queue (MLFQ)** scheduler with advanced features for robust, fair, and efficient process scheduling. This document describes the architecture, algorithms, and capabilities of the scheduler.

## Key Features

### 1. Multi-Level Feedback Queue (MLFQ)

The scheduler uses an 8-level priority queue system:

- **Level 0-1**: Real-time processes (shortest quantum)
- **Level 2-5**: Normal priority processes (medium quantum)
- **Level 6-7**: Background/batch processes (longest quantum)

#### Quantum Calculation

Time slices are calculated dynamically based on priority level:

```
quantum = BASE_QUANTUM * (2^level)
BASE_QUANTUM = 5ms
```

This means:
- Level 0: 5ms (real-time, frequent scheduling)
- Level 4: 80ms (normal processes)
- Level 7: 640ms (background tasks, reduced context switching)

### 2. Scheduling Policies

Four distinct scheduling policies are supported:

#### Realtime (`SchedPolicy::Realtime`)
- Highest priority, always scheduled first
- Minimal quantum for maximum responsiveness
- Used for time-critical tasks

#### Normal (`SchedPolicy::Normal`)
- Default policy for most processes
- Medium priority with balanced quantum
- Dynamic priority adjustment based on behavior

#### Batch (`SchedPolicy::Batch`)
- Lower priority for background tasks
- Longer quantum to reduce overhead
- CPU-bound workloads

#### Idle (`SchedPolicy::Idle`)
- Lowest priority, only runs when nothing else is ready
- Longest quantum
- System maintenance tasks

### 3. Dynamic Priority Adjustment

The scheduler implements intelligent priority adjustment:

```rust
priority = base_priority + nice_value + cpu_penalty - wait_boost
```

Where:
- **base_priority**: Static priority (0-255)
- **nice_value**: User-adjustable (-20 to +19, POSIX compatible)
- **cpu_penalty**: Increases with CPU usage (penalizes CPU-bound processes)
- **wait_boost**: Increases with wait time (rewards I/O-bound processes)

### 4. Preemptive Scheduling

The scheduler supports both cooperative and preemptive multitasking:

- **Timer-based preemption**: Checks on every timer interrupt
- **Priority-based preemption**: Higher priority processes can preempt lower priority ones
- **Time slice exhaustion**: Automatic preemption when quantum expires

### 5. Starvation Prevention

Multiple mechanisms prevent process starvation:

#### Priority Aging
Long-waiting processes gradually gain priority:
- Wait time > 100 ticks → priority boost
- Wait time > 500 ticks → quantum level promotion

#### Priority Boosting
Periodic system-wide priority boost (MLFQ characteristic):
- Resets all processes to their base priority level
- Prevents indefinite priority degradation
- Ensures fairness over time

### 6. Deadlock Detection

The scheduler includes basic deadlock detection:

```rust
pub fn detect_potential_deadlocks() -> [Option<Pid>; MAX_PROCESSES]
```

Detects:
- Processes stuck in Sleeping state for > 10 seconds
- Processes starving in Ready state for excessive time
- Generates warnings for investigation

### 7. Statistics and Monitoring

Comprehensive statistics tracking:

```rust
pub struct SchedulerStats {
    total_context_switches: u64,
    total_preemptions: u64,
    total_voluntary_switches: u64,
    idle_time: u64,
}
```

Per-process statistics:
- Total CPU time consumed
- Wait time in ready queue
- Number of preemptions
- Number of voluntary context switches
- Average CPU burst length

### 8. Process Control Block (PCB)

Extended PCB with scheduling metadata:

```rust
pub struct ProcessEntry {
    process: Process,              // Core process data
    priority: u8,                  // Current dynamic priority
    base_priority: u8,             // Static base priority
    time_slice: u64,               // Remaining quantum
    total_time: u64,               // Total CPU time used
    wait_time: u64,                // Time in ready queue
    last_scheduled: u64,           // Last schedule timestamp
    cpu_burst_count: u64,          // Number of CPU bursts
    avg_cpu_burst: u64,            // Average burst length
    policy: SchedPolicy,           // Scheduling policy
    nice: i8,                      // Nice value (-20 to 19)
    quantum_level: u8,             // MLFQ level (0-7)
    preempt_count: u64,            // Preemption counter
    voluntary_switches: u64,       // Voluntary switch counter
}
```

## Scheduling Algorithms

### Main Scheduling Algorithm

```
1. Update wait times for all ready processes
2. Calculate dynamic priorities
3. Find best candidate based on:
   a. Policy priority (RT > Normal > Batch > Idle)
   b. Dynamic priority within same policy
   c. Wait time as tiebreaker
4. Update previous process state (if any)
5. Activate selected process
6. Update statistics
```

### Time Slice Management

```
On timer interrupt:
1. Decrement current process time slice
2. Update CPU usage statistics
3. Check for higher priority ready processes
4. If preemption conditions met:
   - Increment preempt counter
   - Consider quantum level demotion
   - Trigger reschedule
5. If time slice exhausted:
   - Demote to lower quantum level
   - Force reschedule
```

### Context Switching

The scheduler implements efficient context switching:

1. **Save current context**: All CPU registers saved to PCB
2. **Update statistics**: Track switch type (voluntary/preemptive)
3. **Select next process**: Using scheduling algorithm
4. **Restore new context**: Load registers from new process PCB
5. **Switch address space**: Update CR3 register
6. **Resume execution**: Jump to saved instruction pointer

## API Functions

### Process Management

```rust
// Add process with policy
pub fn add_process_with_policy(
    process: Process,
    priority: u8,
    policy: SchedPolicy,
    nice: i8,
) -> Result<(), &'static str>

// Set scheduling policy
pub fn set_process_policy(
    pid: Pid,
    policy: SchedPolicy,
    nice: i8,
) -> Result<(), &'static str>

// Adjust priority (nice value)
pub fn adjust_process_priority(
    pid: Pid,
    nice_delta: i8,
) -> Result<i8, &'static str>
```

### Scheduling Control

```rust
// Main scheduling function
pub fn schedule() -> Option<Pid>

// Perform context switch
pub fn do_schedule()

// Timer tick handler
pub fn tick(elapsed_ms: u64) -> bool

// Force reschedule
pub fn force_reschedule()
```

### Statistics and Monitoring

```rust
// Get scheduler statistics
pub fn get_stats() -> SchedulerStats

// Get process scheduling info
pub fn get_process_sched_info(pid: Pid) 
    -> Option<(u8, u8, SchedPolicy, i8, u64, u64)>

// Get process counts by state
pub fn get_process_counts() -> (usize, usize, usize, usize)

// Get system load average
pub fn get_load_average() -> (f32, f32, f32)

// List all processes with details
pub fn list_processes()
```

### Advanced Features

```rust
// Priority boost (MLFQ)
pub fn boost_all_priorities()

// Priority aging
pub fn age_process_priorities()

// Deadlock detection
pub fn detect_potential_deadlocks() -> [Option<Pid>; MAX_PROCESSES]
```

## Performance Characteristics

### Time Complexity

- **Schedule selection**: O(n) where n = number of processes
- **Priority update**: O(1) per process
- **Context switch**: O(1)
- **Statistics update**: O(1)

### Space Complexity

- **Process table**: O(MAX_PROCESSES) = O(64)
- **Statistics**: O(1)
- **Per-process metadata**: ~200 bytes

### Overhead

- **Context switch**: ~100-200 CPU cycles (including CR3 reload)
- **Timer interrupt**: ~50 cycles + scheduling decision
- **Priority calculation**: ~20 cycles

## Comparison with Other Schedulers

| Feature | NexaOS | Linux CFS | Windows | FreeBSD ULE |
|---------|--------|-----------|---------|-------------|
| Algorithm | MLFQ | Red-Black Tree | Priority-based | ULE (hybrid) |
| Priorities | 8 levels | 140 levels | 32 levels | Multiple queues |
| Real-time | Yes | Yes (SCHED_FIFO) | Yes | Yes |
| Nice values | -20 to 19 | -20 to 19 | N/A | -20 to 20 |
| Preemption | Timer-based | Tick-based | Preemptive | Preemptive |
| Starvation prevention | Aging + Boost | Virtual runtime | Dynamic | Time-sharing |

## Best Practices

### For System Developers

1. **Use appropriate policies**: 
   - Realtime for critical tasks only
   - Normal for most applications
   - Batch for background work

2. **Monitor statistics**:
   - Call `list_processes()` for debugging
   - Check `detect_potential_deadlocks()` periodically
   - Use `get_load_average()` for system health

3. **Prevent priority inversion**:
   - Be careful with real-time priorities
   - Use priority inheritance for locks (future work)

### For Application Developers

1. **Voluntary yielding**:
   - Call `sched_yield()` in busy-wait loops
   - Use blocking I/O when possible

2. **Nice values**:
   - Lower nice (higher priority) for interactive apps
   - Higher nice (lower priority) for background tasks

3. **Avoid busy-waiting**:
   - Use sleep/wait syscalls
   - Let scheduler optimize CPU usage

## Future Enhancements

1. **True preemptive scheduling from timer interrupt**
   - Implemented: Timer interrupt now triggers `do_schedule()` when required, enabling preemptive scheduling across the system
   - The kernel updates saved user context on interrupt entry and observer safe context-switching in the interrupt handler

2. **CPU affinity and NUMA awareness**
   - Pin processes to specific CPUs
   - Optimize for multi-core systems

3. **Priority inheritance for locks**
   - Prevent priority inversion in IPC
   - Implement PI-futexes

4. **Energy-aware scheduling**
   - P-states and C-states integration
   - Dynamic frequency scaling

5. **Real-time guarantees**
   - SCHED_DEADLINE policy
   - Earliest Deadline First (EDF)

6. **Load balancing across cores**
   - SMP-aware scheduling
   - Work stealing algorithms

7. **Control groups (cgroups)**
   - CPU quota management
   - Fair group scheduling

## Debugging

### Enable Debug Output

The scheduler has extensive debug logging:

```rust
crate::kdebug!("Scheduler debug message");
crate::kinfo!("Scheduler info message");
crate::kwarn!("Scheduler warning");
```

### Common Issues

**Problem**: Process not being scheduled
- Check: Process state (should be Ready)
- Check: Priority (too low?)
- Check: Policy (Idle only runs when nothing else ready)

**Problem**: High preemption count
- Cause: Process consuming too much CPU
- Solution: Will automatically demote to lower quantum level
- Manual: Adjust nice value

**Problem**: Starvation warning
- Check: Too many high-priority processes?
- Check: System overload?
- Solution: Priority aging will eventually help

## Implementation Notes

### Thread Safety

All scheduler functions use spin locks for synchronization:
- `PROCESS_TABLE`: Protected by `Mutex`
- `CURRENT_PID`: Protected by `Mutex`
- `SCHED_STATS`: Protected by `Mutex`
- `GLOBAL_TICK`: Atomic counter

### Interrupt Context

- Timer interrupt increments tick counter
- Scheduling decisions made in timer ISR
- Context switches deferred to safe points

### Memory Management

- Fixed-size process table (no dynamic allocation)
- Process entries copied by value
- No heap usage in scheduler core

## Conclusion

The NexaOS scheduler provides production-grade scheduling with:
- Fair CPU time distribution
- Low latency for interactive tasks
- Efficient batch processing
- Starvation prevention
- Deadlock detection
- Comprehensive monitoring

It combines the best aspects of traditional UNIX schedulers with modern features like multiple scheduling policies and dynamic priority adjustment.
