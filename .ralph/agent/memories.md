# Memories

## Patterns

### mem-1774200465-72c4
> ap clippy rules: main.rs and Cargo.toml both deny clippy::unwrap_used and clippy::expect_used in production code. Use unwrap_or_default(), ok(), ? operator. Test modules use #[allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
<!-- tags: ap, clippy | created: 2026-03-22 -->

### mem-1774200462-57eb
> ap config overlay pattern: overlay_from_table() in config.rs manually handles each TOML section. New config sub-structs require a new block in this function following the pattern: get table section, try_into SubConfig, check contains_key per field before setting.
<!-- tags: ap, config | created: 2026-03-22 -->

### mem-1774200462-57eb
> ap/src/turn.rs has MockProvider and ErrorProvider structs in the test module that implement Provider trait - both must be updated when Provider::stream_completion signature changes
<!-- tags: ap, testing, provider | created: 2026-03-22 -->

## Decisions

## Fixes

## Context
