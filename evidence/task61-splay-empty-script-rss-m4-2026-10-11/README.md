# Splay RSS floor input: empty-script Quench process on M4

Host: Mac16,10, Apple M4, macOS 26.5, arm64, 10 logical CPUs, 16 GiB. Eleven `quench-node -e ""` processes were measured with `/usr/bin/time -l` using the pinned production binary whose SHA-256 is recorded in the JSON. The raw time reports and pre/post host load are in `empty-script-quench-node-11.json`.

Median maximum RSS was **19,038,208 B** (18.16 MiB); the range was **18,530,304–19,202,048 B**. This measures the empty JavaScript process only. It excludes V8-v7 fixture parsing, the harness, and benchmark `Setup`, so it is the process-floor input, not the Splay K=0 result.

Against the latest M4 Bun Splay stock RSS bar of 61,898,752 B, this leaves an optimistic **42,860,544 B** for everything beyond Quench's empty-script process. The GC18 record estimate with int32 inline array elements and a 1.5× live-set high-water is 49,176,528 B. It also needs a slot-to-record locator: even an optimistic 4 B per identity slot adds 4,610,564 B at 1.5× live slots. Records plus locators total 53,787,092 B; with the measured empty-process floor the lower-bound projection is 72,825,300 B, **10.93 MB above** Bun's bar before allocator/block metadata or fixture/harness overhead. Thus those levers alone do not establish an RSS win; the residual and setup costs must also fall or the high-water target must be lower.

For distinction, the latest b72 fixed-work report measured Quench's Splay setup-only process at 65,568,768 B. That includes fixture loading and `Setup`, and is not comparable to the empty-script floor or the stock max-RSS bar as a standalone qualification result.
