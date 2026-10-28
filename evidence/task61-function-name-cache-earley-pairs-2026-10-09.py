#!/usr/bin/env python3
"""11 alternating production pairs for the function-name cache on EarleyBoyer."""
import csv, hashlib, re, subprocess
from pathlib import Path
ROOT=Path.cwd()
BASELINE=ROOT/"target/iteration/task61-function-source-cache-production-20261009/production/quench-node"
CANDIDATE=ROOT/"target/iteration/task61-function-name-cache-20261009/production/quench-node"
FIXTURE=ROOT/"target/iteration/task61-argument-shapes-profile-20261009/materialized-earley-boyer.js"
OUTPUT=ROOT/"target/iteration/task61-function-name-cache-20261009/earley-pairs.csv"
EXPECTED={BASELINE:"cf3916764bd3b59ac3dcb91bd9787cb31ec01537245eaa785ae1cd9e7d6d1d57",CANDIDATE:"278d8f2a92b76c123814ff39ecbaae723171025810e18845ede76d8903c76885",FIXTURE:"aa379c1d54f5d13de32ebf2b50729d0cbc64bb23de5e524256c7b2270213cc0b"}
I=re.compile(r"^\s*(\d+)\s+instructions retired\s*$",re.M)
R=re.compile(r"^\s*(\d+)\s+maximum resident set size\s*$",re.M)
T=re.compile(r"^\s*([\d.]+) real\s+([\d.]+) user\s+([\d.]+) sys\s*$",re.M)
S=re.compile(r"^Score:\s*(-?[\d.]+)\s*$",re.M)
B=re.compile(r"^__quenchBenchResult: (.*?): (-?[\d.]+)\s*$",re.M)
sha=lambda b:hashlib.sha256(b).hexdigest()
def normalize(s):
 s=B.sub(r"__quenchBenchResult: \1: <score>",s)
 return "\n".join(x for x in s.splitlines() if x.strip()!="----" and not x.startswith("Score:"))
for path,expected in EXPECTED.items():
 actual=sha(path.read_bytes())
 if actual!=expected: raise SystemExit(f"hash mismatch {path}: expected {expected}, got {actual}")
rows=[]
for pair in range(1,12):
 order=("candidate","baseline") if pair%2 else ("baseline","candidate")
 data={}
 for label in order:
  binary=CANDIDATE if label=="candidate" else BASELINE
  result=subprocess.run(["/usr/bin/time","-l",str(binary),str(FIXTURE)],text=True,capture_output=True)
  mi,mr,mt,ms=I.search(result.stderr),R.search(result.stderr),T.search(result.stderr),S.search(result.stdout)
  if not all((mi,mr,mt,ms)): raise SystemExit(f"missing parsed output pair={pair} {label} exit={result.returncode}: {result.stdout!r} {result.stderr!r}")
  elapsed,user,system=map(float,mt.groups()); normalized=normalize(result.stdout)
  data[label]={"exit":result.returncode,"score":float(ms.group(1)),"instructions":int(mi.group(1)),"max_rss_bytes":int(mr.group(1)),"elapsed_seconds":elapsed,"user_seconds":user,"system_seconds":system,"stdout":result.stdout,"stdout_sha256":sha(result.stdout.encode()),"normalized_stdout":normalized,"normalized_stdout_sha256":sha(normalized.encode()),"time_stderr":result.stderr}
  if result.returncode: raise SystemExit(f"nonzero exit pair={pair} {label}: {result.returncode}")
 row={"pair":pair,"order":">".join(order),"baseline_binary_sha256":EXPECTED[BASELINE],"candidate_binary_sha256":EXPECTED[CANDIDATE],"fixture_sha256":EXPECTED[FIXTURE]}
 for label in ("baseline","candidate"):
  for field in ("score","instructions","max_rss_bytes","elapsed_seconds","user_seconds","system_seconds","exit","stdout_sha256","normalized_stdout_sha256","stdout","normalized_stdout","time_stderr"): row[f"{label}_{field}"]=data[label][field]
 row["normalized_outputs_match"]=data["baseline"]["normalized_stdout"]==data["candidate"]["normalized_stdout"]
 rows.append(row); print(f"pair {pair}/11 complete",flush=True)
fields=["pair","order","baseline_binary_sha256","candidate_binary_sha256","fixture_sha256"]
for label in ("baseline","candidate"):
 fields += [f"{label}_{field}" for field in ("score","instructions","max_rss_bytes","elapsed_seconds","user_seconds","system_seconds","exit","stdout_sha256","normalized_stdout_sha256","stdout","normalized_stdout","time_stderr")]
fields.append("normalized_outputs_match")
with OUTPUT.open("w",newline="") as stream:
 writer=csv.DictWriter(stream,fieldnames=fields); writer.writeheader(); writer.writerows(rows)
print(f"csv={OUTPUT}")
