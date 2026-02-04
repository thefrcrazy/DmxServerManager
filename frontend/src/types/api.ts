// Standardized API response types

// Base response types
export interface ApiResponse<T> {
  data: T;
  success: boolean;
  timestamp: string;
}

export interface ApiError {
  error: string;
  code?: string;
  details?: any;
}

// Auth responses
export interface AuthResponse {
  token: string;
  user: UserInfo;
}

export interface UserInfo {
  id: string;
  username: string;
  role: string;
  accent_color?: string;
}

export interface SetupStatus {
  needs_setup: boolean;
}

// Server responses
export interface Server {
  id: string;
  name: string;
  game_type: string;
  executable_path: string;
  working_dir: string;
  java_path?: string;
  min_memory?: string;
  max_memory?: string;
  extra_args?: string;
  config?: any;
  auto_start: boolean;
  created_at: string;
  updated_at: string;

  // New settings (formerly manager.json)
  backup_enabled: boolean;
  backup_frequency: number;
  backup_max_backups: number;
  backup_prefix: string;

  discord_username?: string;
  discord_avatar?: string;
  discord_webhook_url?: string;
  discord_notifications?: any;

  logs_retention_days: number;
  watchdog_enabled: boolean;

  auth_mode: string;
  bind_address: string;
  port: number;

  // Runtime status
  status?: string;
  cpu_usage?: number;
  memory_usage?: number;
  players_online?: number;
  players_max?: number;
}

export interface ServerStats {
  cpu: number;
  memory: number;
  players: number;
  status: string;
}

// System responses
export interface SystemStatsResponse {
  cpu: number;
  ram: number;
  ram_used: number;
  ram_total: number;
  disk: number;
  disk_used: number;
  disk_total: number;
  players_current: number;
  players_max: number;
  cpu_cores: number;
  managed_cpu: number;
  managed_cpu_normalized: number;
  managed_ram: number;
  managed_disk: number;
}

export interface JavaVersion {
  path: string;
  version: string;
}

// Backup responses
export interface Backup {
  id: string;
  server_id: string;
  filename: string;
  size_bytes: number;
  created_at: string;
}

// Settings responses
export interface AppSettings {
  version: string;
  servers_dir: string;
  backups_dir: string;
  webhook_url?: string;
}

// Standardized API methods
export interface StandardApiMethods<T> {
  getAll(): Promise<ApiResponse<T[]>>;
  getById(id: string): Promise<ApiResponse<T>>;
  create(data: Partial<T>): Promise<ApiResponse<T>>;
  update(id: string, data: Partial<T>): Promise<ApiResponse<T>>;
  delete(id: string): Promise<ApiResponse<{ success: boolean }>>;
}

// Console responses
export interface ConsoleOutput {
  server_id: string;
  output: string;
  timestamp: string;
}

// Player responses
export interface PlayerInfo {
  server_id: string;
  player_name: string;
  first_seen: string;
  last_seen: string;
  is_online: boolean;
}

// Metrics responses
export interface MetricDataPoint {
  id: string;
  server_id: string;
  cpu_usage: number;
  memory_bytes: number;
  disk_bytes: number;
  player_count: number;
  recorded_at: string;
}

export interface MetricsHistoryResponse {
  server_id: string;
  period: string;
  data: MetricDataPoint[];
}