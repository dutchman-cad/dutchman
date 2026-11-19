#!/usr/bin/env python3
"""Link ARM CI tests with LLD and record individual link durations."""
import json
import os
from pathlib import Path
import subprocess
import sys
import time


def main():
    if sys.argv[1:2] == ["--summary"]:
        timings = [json.loads(p.read_text()) for p in Path(sys.argv[2]).glob("*.json")]
        print(f"Links: {len(timings)}; total link seconds: {sum(t['seconds'] for t in timings):.2f}")
        for timing in sorted(timings, key=lambda t: t["seconds"], reverse=True)[:10]:
            print(f"{timing['seconds']:.2f}s {timing['output']}")
        return 0
    args = sys.argv[1:]
    output = next((args[i + 1] for i, arg in enumerate(args[:-1]) if arg == "-o"), "unknown")
    started = time.monotonic()
    result = subprocess.run(["cc", *args, "-fuse-ld=lld"], check=False)
    folder = Path(os.environ["DUTCHMAN_LINK_TIMES"])
    folder.mkdir(parents=True, exist_ok=True)
    (folder / f"{os.getpid()}.json").write_text(json.dumps({
        "linker": "lld", "output": output, "seconds": time.monotonic() - started, "status": result.returncode,
    }))
    return result.returncode


if __name__ == "__main__":
    sys.exit(main())
