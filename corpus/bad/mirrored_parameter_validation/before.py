def validate_parameters(raw: Mapping[str, Any]) -> None:
    if not isinstance(raw, Mapping):
        raise ParameterValidationError(None, "parameters must be a mapping")
    for key, value in raw.items():
        # bits:allow(BCS007): Python has no brace blocks
        if not isinstance(key, str) or not key:
            raise ParameterValidationError(
                None, "parameter names must be non-empty strings"
            )
        if key in RESERVED_PARAMETER_KEYS:
            raise ParameterValidationError(
                key, f"parameter '{key}' is reserved and managed by nextloop"
            )
        if value is None:
            continue
        if key == "temperature":
            if not _is_number(value) or not math.isfinite(value) or value < 0:
                raise ParameterValidationError(
                    key, "temperature must be a finite number >= 0"
                )
        elif key == "top_p":
            if not _is_number(value) or not math.isfinite(value) or not 0 <= value <= 1:
                raise ParameterValidationError(
                    key, "top_p must be a finite number between 0 and 1"
                )
        elif key == "top_k":
            if not _is_int(value) or value < 1:
                raise ParameterValidationError(
                    key, "top_k must be an integer >= 1"
                )
        elif key in ("max_tokens", "max_completion_tokens"):
            if not _is_int(value) or value < 1:
                raise ParameterValidationError(
                    key, f"{key} must be an integer >= 1"
                )
        elif key == "seed":
            if not _is_int(value):
                raise ParameterValidationError(key, "seed must be an integer")
        elif key in ("presence_penalty", "frequency_penalty"):
            if not _is_number(value) or not math.isfinite(value) or not -2 <= value <= 2:
                raise ParameterValidationError(
                    key, f"{key} must be a finite number between -2 and 2"
                )
        elif key == "stop":
            if not (isinstance(value, str) and value != "") and not _valid_string_list(value):
                raise ParameterValidationError(
                    key, "stop must be a non-empty string or list"
                )
        elif key == "stop_sequences" and not _valid_string_list(value):
            raise ParameterValidationError(
                key, "stop_sequences must be a list of non-empty strings"
            )
    if raw.get("max_tokens") is not None and raw.get("max_completion_tokens") is not None:
        raise ParameterValidationError(
            "max_completion_tokens",
            "max_tokens and max_completion_tokens are mutually exclusive",
        )
