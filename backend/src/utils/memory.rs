pub fn parse_memory_to_bytes(mem: &str) -> u64 {
    let mem = mem.to_uppercase();
    let num_part: String = mem.chars().take_while(|c| c.is_digit(10)).collect();
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

pub fn calculate_smart_heap(total_bytes: u64) -> (String, String) {
    let one_gb = 1024 * 1024 * 1024;
    
    // Xmx: Target - 1G (or 1200M for 2G)
    let xmx_bytes = if total_bytes <= 2 * one_gb {
        std::cmp::min(1200 * 1024 * 1024, total_bytes.saturating_sub(256 * 1024 * 1024))
    } else {
        total_bytes.saturating_sub(one_gb)
    };

    // Xms: 2/3 of target (or 1/2 for 2G)
    let xms_bytes = if total_bytes <= 2 * one_gb {
        total_bytes / 2
    } else {
        (total_bytes * 2) / 3
    };
    
    // Ensure xms <= xmx
    let xms_bytes = std::cmp::min(xms_bytes, xmx_bytes);

    (
        format!("{}M", xms_bytes / (1024 * 1024)),
        format!("{}M", xmx_bytes / (1024 * 1024))
    )
}

pub fn calculate_heap_bytes(total_bytes: u64) -> u64 {
    let one_gb = 1024 * 1024 * 1024;
    if total_bytes <= 2 * one_gb {
        std::cmp::min(1200 * 1024 * 1024, total_bytes.saturating_sub(256 * 1024 * 1024))
    } else {
        total_bytes.saturating_sub(one_gb)
    }
}
