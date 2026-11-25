# bits:spec(pure)
def normalize_config(
    base: dict[str, int],
    overrides: dict[str, int],
) -> dict[str, int]:
    return base | overrides


def load_normalized(path: str, overrides: dict[str, int]) -> dict[str, int]:
    with open(path) as handle:
        base = {line: 1 for line in handle.read().splitlines()}
    return normalize_config(base, overrides)
