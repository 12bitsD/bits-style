# bits:spec(pure)
def normalize_config(path: str, overrides: dict[str, int]) -> dict[str, int]:
    merged = read_file(path)
    merged.update(overrides)
    return merged


def read_file(path: str) -> dict[str, int]:
    with open(path) as handle:
        return {line: 1 for line in handle.read().splitlines()}
