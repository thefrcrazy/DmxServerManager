pub fn parse_memory_to_bytes(mem: &str) -> u64 {
    let mem = mem.to_uppercase();
    let num_part: String = mem.chars().take_while(|c| c.is_ascii_digit()).collect();
    let val = num_part.parse::<u64>().unwrap_or(4);
    
    if mem.ends_with('G') {
        val * 1024 * 1024 * 1024
    } else if mem.ends_with('M') {
        val * 1024 * 1024
    } else if mem.ends_with('K') {
        val * 1024
    } else {
        // Assume G if no unit provided for safety/compatibility
        val * 1024 * 1024 * 1024
    }
}

pub fn calculate_jvm_tokens(heap_bytes: u64) -> (String, String) {
    let xmx_bytes = heap_bytes;
    
    // Xms: 2/3 of Xmx for stability, or 1/2 for small servers (<2GB)
    let one_gb = 1024 * 1024 * 1024;
    let xms_bytes = if heap_bytes <= 2 * one_gb {
        heap_bytes / 2
    } else {
        (heap_bytes * 2) / 3
    };

    (
        format!("{}M", xms_bytes / (1024 * 1024)),
        format!("{}M", xmx_bytes / (1024 * 1024))
    )
}

pub fn calculate_overhead(heap_bytes: u64) -> u64 {
    let one_gb = 1024 * 1024 * 1024;
    
    // Overhead estimation:
    // For small heap (<2GB), we expect ~800MB overhead
    // For larger heap, we expect ~1GB overhead
    if heap_bytes <= 2 * one_gb {
        800 * 1024 * 1024 // 0.8 GB
    } else {
        one_gb // 1.0 GB
    }
}

pub fn calculate_total_memory(heap_bytes: u64) -> u64 {
    heap_bytes + calculate_overhead(heap_bytes)
}
