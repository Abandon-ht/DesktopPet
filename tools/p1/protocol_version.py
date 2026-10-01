"""Read the workspace wire version so native smoke checks track the real host."""
from pathlib import Path
import re

source = Path(__file__).resolve().parents[2] / "crates/pet-protocol/src/lib.rs"
match = re.search(r"pub const PROTOCOL_VERSION: u32 = (\d+);", source.read_text())
if not match:
    raise RuntimeError("cannot read workspace protocol version")
VERSION = int(match.group(1))
