#!/usr/bin/env python3
"""Combine clean per-fixture rows and derive the M4 Splay candidate guard."""
import hashlib, json, random, statistics
from pathlib import Path
HERE=Path(__file__).parent
N=100_000
LOWER,UPPER=2_499,97_499
SEED=61_101_013
FIELDS=("wall_ns","cycles","instructions")
RSS="peak_rss_bytes"
BASE="quench_baseline"
CAND="quench_candidate"

def read(name):
    return json.loads((HERE/name).read_text())

def digest(name):
    return hashlib.sha256((HERE/name).read_bytes()).hexdigest()

def clean(sample):
    return sample["status"]==0 and sample["contention"].get("clean") is True

def deltas(report, key):
    record=report["fixtures"][key]
    divisor=record["plan"]["benchmark_count"]*record["plan"]["iterations_per_benchmark"]
    result={field:[] for field in (*FIELDS,RSS)}
    for rnd in record["rounds"]:
        b,c=rnd["samples"][BASE],rnd["samples"][CAND]
        bs,cs=rnd["setup_only_samples"][BASE],rnd["setup_only_samples"][CAND]
        samples=(b,c,bs,cs)
        if not all(s["status"]==0 and clean(s) for s in samples):
            continue
        for field in FIELDS:
            bp=(b[field]-bs[field])/divisor
            cp=(c[field]-cs[field])/divisor
            result[field].append(100*(cp/bp-1))
        result[RSS].append(100*(c["peak_rss_bytes"]/b["peak_rss_bytes"]-1))
    return result

def interval(values, seed):
    rng=random.Random(seed); n=len(values); draws=[]
    for _ in range(N):
        draws.append(statistics.median(values[rng.randrange(n)] for _ in range(n)))
    draws.sort()
    return [draws[LOWER],draws[UPPER]]

def summarize(data, offset):
    return {f:{"clean_pairs":len(v),"median_delta_percent":statistics.median(v),"paired_bootstrap_95_percentile_interval_percent":interval(v,SEED+offset+i)} for i,(f,v) in enumerate(data.items())}

def main():
    aa=read("all-eight-aa-11.json")
    ab=read("all-eight-ab-11.json")
    regexp=read("regexp-ab-topup-11.json")
    richards=read("richards-ab-topup-11.json")
    splay=read("splay-ab-11.json")
    for source in (regexp,richards,splay):
        key=next(iter(source["fixtures"]))
        ab["fixtures"][key]=source["fixtures"][key]
    out={"schema":1,"platform":"M4 arm64 macOS, Mac16,10","method":"Each candidate fixture uses only clean paired rows; wall/cycles/instructions are per-run marginal values after subtracting K=0; RSS is raw work max RSS. Bootstrap uses 100,000 paired median resamples with fixed seed. Other-fixture point losses subtract that fixture's A/A upper 95% endpoint; caps are 2% wall and 1% RSS.","bootstrap_resamples":N,"bootstrap_seed":SEED,"fixtures":{},"reports":{}}
    keys={k.split("/")[-1]:k for k in ab["fixtures"]}
    for basename,key in sorted(keys.items()):
        akey=keys[basename]
        ad=deltas(aa,akey); bd=deltas(ab,key)
        asum=summarize(ad,0); bsum=summarize(bd,100)
        out["fixtures"][basename.removesuffix(".js")]={"aa":asum,"ab":bsum,"aa_adjusted_point_losses":{"wall_percent":max(0,bsum["wall_ns"]["median_delta_percent"]-asum["wall_ns"]["paired_bootstrap_95_percentile_interval_percent"][1]),"rss_percent":max(0,bsum[RSS]["median_delta_percent"]-asum[RSS]["paired_bootstrap_95_percentile_interval_percent"][1])}}
    names=["all-eight-aa-11.json","all-eight-ab-11.json","regexp-ab-topup-11.json","richards-ab-topup-11.json","splay-aa-11.json","splay-ab-11.json"]
    out["reports"]={name:digest(name) for name in names}
    out["clean_pairs"]={name:{"aa":v["aa"]["wall_ns"]["clean_pairs"],"ab":v["ab"]["wall_ns"]["clean_pairs"]} for name,v in out["fixtures"].items()}
    caps={"wall_percent":2,"rss_percent":1}
    out["guard"]={"non_splay_point_screen_passes":all(v["aa_adjusted_point_losses"]["wall_percent"]<=caps["wall_percent"] and v["aa_adjusted_point_losses"]["rss_percent"]<=caps["rss_percent"] for n,v in out["fixtures"].items() if n!="splay"),"all_eight_aa_11_clean":all(v["aa"]["wall_ns"]["clean_pairs"]==11 for v in out["fixtures"].values()),"all_eight_ab_11_clean":all(v["ab"]["wall_ns"]["clean_pairs"]==11 for v in out["fixtures"].values()),"caps_after_aa_upper_95_allowance":caps,"splay_wall_gain_resolved":out["fixtures"]["splay"]["ab"]["wall_ns"]["paired_bootstrap_95_percentile_interval_percent"][1]<0,"splay_rss_gain_resolved":out["fixtures"]["splay"]["ab"][RSS]["paired_bootstrap_95_percentile_interval_percent"][1]<0}
    if not out["guard"]["non_splay_point_screen_passes"]:
        richards_loss=out["fixtures"]["richards"]["aa_adjusted_point_losses"]["wall_percent"]
        out["guard"]["decision"]=(
            f"M4 guard fails: Richards wall loss after its A/A upper-95% allowance "
            f"is {richards_loss:.3f}% against a 2.000% cap. Splay wall and RSS gains "
            "are resolved, but this candidate is not mergeable until the guard is "
            "fixed. The stock diagnostic is Deno-contended and Linux has not "
            "measured this candidate."
        )
    else:
        out["guard"]["decision"]=(
            "M4 other-fixture point screen passes; Splay wall/RSS improvement is "
            "resolved. This is a candidate result only and does not establish the "
            "stock win or Linux result."
        )
    (HERE/"all-eight-analysis.json").write_text(json.dumps(out,indent=2)+"\n")
    for name,v in out["fixtures"].items():
        w=v["ab"]["wall_ns"]; r=v["ab"][RSS]; adj=v["aa_adjusted_point_losses"]
        print(f"{name}: n={w['clean_pairs']}, wall {w['median_delta_percent']:+.3f}% [{w['paired_bootstrap_95_percentile_interval_percent'][0]:+.3f}, {w['paired_bootstrap_95_percentile_interval_percent'][1]:+.3f}], RSS {r['median_delta_percent']:+.3f}% [{r['paired_bootstrap_95_percentile_interval_percent'][0]:+.3f}, {r['paired_bootstrap_95_percentile_interval_percent'][1]:+.3f}], adjusted {adj['wall_percent']:.3f}%/{adj['rss_percent']:.3f}%")
    print("guard",out["guard"])
if __name__=="__main__":main()
