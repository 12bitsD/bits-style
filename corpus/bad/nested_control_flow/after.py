def collect_ready(batches: list[list[int]] | None) -> list[int]:
    if not batches:
        return []
    return [item for batch in batches for item in batch if is_valid(item)]
