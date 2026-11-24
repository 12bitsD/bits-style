async def run_command(command: str, harness: Harness) -> object:
    if command == "tools":
        await harness.check()
        return harness.tools.catalog()
    elif command == "skills":
        await harness.check()
        if harness.skills is None:
            raise NextloopError("skills not configured")
        return harness.skills.list()
    else:
        await harness.check()
        return harness.status()
