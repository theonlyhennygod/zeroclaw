# ZeroClaw Concurrency Architecture

ZeroClaw 并发架构实现了高性能异步消息处理系统，集成了 Worker Pool、背压机制、请求去重和熔断器保护。

## 架构概览

```
┌─────────────────────────────────────────────────────────────────┐
│                     ConcurrentMessageProcessor                  │
├─────────────────────────────────────────────────────────────────┤
│  ┌──────────────┐  ┌──────────────┐  ┌──────────────┐          │
│  │ Deduplicator │  │   Backpressure │  │ CircuitBreaker│         │
│  │   (去重)      │  │    (背压)      │  │   (熔断器)     │         │
│  └──────┬───────┘  └──────┬───────┘  └──────┬───────┘         │
│         └─────────────────┴─────────────────┘                  │
│                           │                                     │
│                    ┌──────────────┐                            │
│                    │  Worker Pool │                            │
│                    │  (任务执行池) │                            │
│                    └──────────────┘                            │
└─────────────────────────────────────────────────────────────────┘
```

## 模块说明

### 1. Worker Pool (`worker_pool.rs`)

类似 OpenClaw 的异步任务执行池：

- **固定工作线程**: 可配置的并发工作线程数
- **优先级队列**: Critical > High > Normal > Low > Background
- **任务超时**: 支持任务级超时控制
- **优雅关闭**: 安全停止所有工作线程

```rust
use zeroclaw::concurrency::{WorkerPool, Task, TaskPriority};

let pool = WorkerPool::new(4, 100); // 4 workers, queue size 100

let task = Task::new(async {
    // 异步任务
    "result"
})
.with_priority(TaskPriority::High)
.with_timeout(Duration::from_secs(30));

let result = pool.submit(task).await;
```

### 2. Backpressure (`backpressure.rs`)

基于 Semaphore 的背压机制：

- **并发控制**: 限制同时处理的请求数
- **速率限制**: 令牌桶算法实现 QPS 限制
- **自适应限流**: 根据延迟动态调整并发

```rust
use zeroclaw::concurrency::{Backpressure, AdaptiveLimiter};

// 基础背压
let bp = Backpressure::new(10); // 最多10个并发
let permit = bp.acquire().await;
// ... 处理请求 ...
drop(permit); // 自动释放

// 自适应限流
let limiter = AdaptiveLimiter::new(
    10,   // 初始并发
    2,    // 最小并发
    50,   // 最大并发
    100,  // 每秒速率
    100,  // 目标延迟(ms)
);
```

### 3. Deduplicator (`deduplicator.rs`)

请求去重，防止重复处理：

- **精确去重**: 基于哈希的精确匹配
- **滑动窗口**: 时间窗口内去重
- **布隆过滤器**: 内存高效的概率去重
- **组合策略**: 多种策略的混合使用

```rust
use zeroclaw::concurrency::{Deduplicator, DedupKey};

let dedup = Deduplicator::new(Duration::from_secs(60));

let key = DedupKey::combine(vec![
    channel.to_string(),
    user.to_string(),
    content_hash.to_string(),
]);

if dedup.check_and_update(&key) {
    // 重复消息，跳过处理
    return;
}
// ... 处理消息 ...
```

### 4. Circuit Breaker (`circuit_breaker.rs`)

熔断器保护，防止级联故障：

- **三态模型**: Closed → Open → HalfOpen → Closed
- **自动恢复**: 超时后进入半开状态探测
- **多触发条件**: 连续失败数或失败率
- **状态回调**: 状态变更通知

```rust
use zeroclaw::concurrency::{CircuitBreaker, CircuitConfig};

let cb = CircuitBreaker::new(CircuitConfig {
    failure_threshold: 5,        // 5次失败触发熔断
    success_threshold: 3,        // 3次成功恢复
    timeout_duration: Duration::from_secs(60),
    failure_rate_threshold: 0.5, // 50%失败率触发
    ..Default::default()
});

if !cb.allow_request() {
    return Err("Service unavailable".into());
}

match do_request().await {
    Ok(result) => {
        cb.record_success();
        Ok(result)
    }
    Err(e) => {
        cb.record_failure();
        Err(e)
    }
}
```

## Channel 系统集成

### MessageProcessor

专为 Channel 系统设计的高级处理器：

```rust
use zeroclaw::concurrency::channel_integration::{
    MessageProcessor, 
    MessageProcessorConfig,
    ConcurrentMessageProcessor
};

// 基础配置
let config = MessageProcessorConfig {
    worker_pool_size: 4,
    max_concurrent_requests: 10,
    enable_dedup: true,
    enable_circuit_breaker: true,
    enable_backpressure: true,
    ..Default::default()
};

// 创建处理器
let (processor, mut responses) = MessageProcessor::new(config);

// 处理消息
let result = processor.process_message(
    channel,
    message,
    |msg| async move {
        // 异步处理逻辑
        provider.chat(&msg.content).await
    }
).await;

// 高级封装
let concurrent = ConcurrentMessageProcessor::new(config);
concurrent.start(channels, provider, system_prompt).await?;
```

## 与现有 Channel 系统集成

### 修改 Channel 启动代码

在 `channels/mod.rs` 的 `start_channels` 函数中，替换直接处理逻辑：

```rust
// 原代码:
while let Some(msg) = rx.recv().await {
    // 直接处理...
}

// 新代码:
let config = MessageProcessorConfig::default();
let (processor, mut response_rx) = MessageProcessor::new(config);

// 消息接收任务
tokio::spawn(async move {
    while let Some(msg) = rx.recv().await {
        let ch = find_channel(&channels, &msg.channel);
        processor.process_message(ch, msg, process_fn).await;
    }
});

// 响应处理任务
while let Some((channel, message, result)) = response_rx.recv().await {
    handle_response(channel, message, result).await;
}
```

## 性能优化

### Worker Pool 优化
- 根据 CPU 核心数设置 worker 数量
- 使用优先级队列处理紧急消息
- 任务超时防止阻塞

### 背压优化
- 自适应限流根据延迟调整
- 监控负载百分比
- 设置合理的目标延迟

### 去重优化
- 组合键策略：`channel + sender + content_hash`
- 合理的 TTL 设置（60s 推荐）
- 定期清理过期条目

### 熔断器优化
- 根据服务特性选择配置（fast_fail/lenient/strict）
- 监控失败率和状态变化
- 启用半开状态自动恢复

## 监控指标

### Worker Pool
- `active_workers`: 活跃工作线程数
- `queued_tasks`: 队列中的任务数
- `completed_tasks`: 已完成任务数
- `timeout_tasks`: 超时任务数
- `avg_processing_time_ms`: 平均处理时间

### Backpressure
- `available_permits`: 可用许可数
- `waiting_count`: 等待中的请求数
- `load_percentage`: 负载百分比
- `rejected_count`: 被拒绝的请求数

### Circuit Breaker
- `state`: 当前状态 (Closed/Open/HalfOpen)
- `failure_rate`: 失败率
- `consecutive_failures`: 连续失败次数
- `time_in_current_state`: 当前状态持续时间

### Deduplicator
- `duplicates_found`: 发现的重复数
- `unique_added`: 新增唯一键数
- `current_entries`: 当前条目数

## 测试

运行所有并发模块测试：

```bash
cargo test --lib concurrency
```

### 基准测试建议

```rust
#[bench]
fn bench_message_throughput(b: &mut Bencher) {
    let processor = MessageProcessor::new(config);
    b.iter(|| {
        // 测试吞吐量
    });
}

#[bench]
fn bench_dedup_performance(b: &mut Bencher) {
    let dedup = Deduplicator::new(Duration::from_secs(60));
    b.iter(|| {
        dedup.check_and_update(&key)
    });
}
```

## 故障排查

### Worker Pool 饱和
- 增加 worker_pool_size
- 检查是否有阻塞操作
- 增加 task_queue_size

### 背压频繁触发
- 增加 max_concurrent_requests
- 检查下游服务延迟
- 启用自适应限流

### 熔断器频繁触发
- 检查服务健康状况
- 调整 failure_threshold
- 检查网络超时设置

### 去重效果不佳
- 检查 DedupKey 设计
- 调整 dedup_ttl
- 考虑使用组合键

## 配置建议

### 小规模部署 (< 100 并发)
```rust
MessageProcessorConfig {
    worker_pool_size: 2,
    max_concurrent_requests: 5,
    dedup_ttl: Duration::from_secs(30),
    ..Default::default()
}
```

### 中规模部署 (100-1000 并发)
```rust
MessageProcessorConfig {
    worker_pool_size: 4,
    max_concurrent_requests: 20,
    dedup_ttl: Duration::from_secs(60),
    ..Default::default()
}
```

### 大规模部署 (> 1000 并发)
```rust
MessageProcessorConfig {
    worker_pool_size: 8,
    max_concurrent_requests: 50,
    dedup_ttl: Duration::from_secs(120),
    circuit_config: CircuitConfig::lenient(),
    ..Default::default()
}
```
