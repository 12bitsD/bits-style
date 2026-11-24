RESERVED_PARAMETER_KEYS = frozenset({"model", "messages", "stream", "tools", "system"})


def validate_parameters(raw: Mapping[str, Any]) -> None:
    if not isinstance(raw, Mapping):
        raise TypeError("parameters must be a mapping")
    reserved = RESERVED_PARAMETER_KEYS.intersection(raw)
    if reserved:
        names = ", ".join(sorted(reserved))
        raise ValueError(f"reserved parameters managed by loop: {names}")
