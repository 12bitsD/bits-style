async def run_command(command: str, harness: Harness) -> object:
    await harness.check()
    if command == "tools":
        return harness.tools.catalog()
    if command == "skills":
        if harness.skills is None:
            raise NextloopError("skills not configured")
        return harness.skills.list()
    return harness.status()
