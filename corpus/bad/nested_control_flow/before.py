def collect_ready(batches: list[list[int]]) -> list[int]:
    ready: list[int] = []
    if batches is not None:
        for batch in batches:
            if batch:
                for item in batch:
                    if not (not is_valid(item)):
                        ready.append(item)
    return ready
