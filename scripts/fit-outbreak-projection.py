#!/usr/bin/env python3
"""Fit outbreak.html's lat/lon -> pixel projection to the territory map SVG.

The US map in index.html (reused by outbreak.html) isn't a stock projection,
so the outbreak page projects member coordinates with a generic conic whose
parameters were fitted to state-corner vertices of the map with known
lat/lon. Re-run this if the map SVG ever changes and paste the printed PROJ
line over the one in outbreak.html. Pure Python, no dependencies.

Model: x = x0 + rho(lat) * sin(n * (lon - lon0)),
       y = y0 - sy * rho(lat) * cos(n * (lon - lon0)),   rho = a + b*phi + c*phi^2
"""
import math
import random
from pathlib import Path

html = (Path(__file__).resolve().parent.parent / "index.html").read_text(encoding="utf-8")
D = math.pi/180
# (lat, lon) -> (x, y) control points read off the state-corner vertices
CP = [
 (41.0,-109.05, 238.8,199.9), (41.0,-102.05, 334.2,209.4), (37.0,-102.04, 329.7,283.4), (36.999,-109.045, 228.8,273.1),
 (45.0,-111.05, 223.3,123.0), (45.0,-104.05, 313.0,134.1), (41.0,-104.05, 306.9,207.4), (41.0,-111.05, 211.7,195.9),
 (42.0,-114.04, 174.7,170.7), (42.0,-111.05, 214.6,177.6), (37.0,-114.05, 157.2,261.3),
 (31.33,-109.05, 214.4,376.8), (32.0,-103.06, 308.0,374.3), (37.0,-103.0, 315.8,282.4), (36.5,-103.0, 314.6,291.5),
 (49.0,-116.05, 175.0,39.9), (49.0,-104.05, 319.4,61.4), (45.94,-104.05, 314.6,116.8), (45.94,-96.56, 410.0,121.3),
 (37.0,-94.62, 437.2,285.9), (40.0,-102.05, 333.0,227.9), (36.5,-100.0, 358.8,294.1), (36.5,-103.0, 315.1,291.5),
 (42.0,-120.0, 96.1,153.0), (42.0,-117.03, 135.2,162.3), (49.0,-117.03, 163.3,37.3), (46.0,-116.92, 152.7,91.0),
 (32.72,-114.72, 129.9,340.8), (39.72,-80.52, 632.3,218.2), (42.0,-75.35, 694.0,163.5),
 (35.0,-88.2, 533.1,318.4), (35.0,-85.6, 571.5,314.9), (31.0,-85.0, 588.9,386.8),
 (49.0,-97.23, 402.2,65.4), (49.0,-95.15, 427.5,65.5), (40.0,-95.31, 426.8,230.5), (43.0,-104.05, 310.0,170.6),
]
def model(p, lat, lon):
    lon0, n, a, b, c, x0, y0, sy = p
    phi = lat*D
    rho = a + b*phi + c*phi*phi
    th = n*(lon - lon0)*D
    return x0 + rho*math.sin(th), y0 - sy*rho*math.cos(th)
def err(p):
    return sum((model(p,la,lo)[0]-x)**2 + (model(p,la,lo)[1]-y)**2 for la,lo,x,y in CP)
def nelder_mead(f, x0, step, iters=20000):
    n = len(x0); pts = [list(x0)]
    for i in range(n):
        q = list(x0); q[i] += step[i]; pts.append(q)
    vals = [f(q) for q in pts]
    for _ in range(iters):
        order = sorted(range(n+1), key=lambda i: vals[i]); pts = [pts[i] for i in order]; vals = [vals[i] for i in order]
        cen = [sum(p[i] for p in pts[:-1])/n for i in range(n)]
        xr = [cen[i] + (cen[i]-pts[-1][i]) for i in range(n)]; fr = f(xr)
        if fr < vals[0]:
            xe = [cen[i] + 2*(cen[i]-pts[-1][i]) for i in range(n)]; fe = f(xe)
            if fe < fr: pts[-1], vals[-1] = xe, fe
            else: pts[-1], vals[-1] = xr, fr
        elif fr < vals[-2]: pts[-1], vals[-1] = xr, fr
        else:
            xc = [cen[i] + 0.5*(pts[-1][i]-cen[i]) for i in range(n)]; fc = f(xc)
            if fc < vals[-1]: pts[-1], vals[-1] = xc, fc
            else:
                for j in range(1, n+1):
                    pts[j] = [pts[0][i] + 0.5*(pts[j][i]-pts[0][i]) for i in range(n)]; vals[j] = f(pts[j])
    i = min(range(n+1), key=lambda i: vals[i]); return pts[i], vals[i]
# initial guess: albers-like; rho ~ 1600 - 1600*phi(rad)... pick from data: at lat 41 rho ~ ? y0 ~ -900
best = None
for trial in range(12):
    random.seed(trial)
    p0 = [-96 + random.uniform(-5,5), 0.6 + random.uniform(-0.2,0.2), 2200 + random.uniform(-500,500), -2000, 0, 480 + random.uniform(-30,30), -900 + random.uniform(-300,300), 1.0 + random.uniform(-0.2,0.2)]
    p, v = nelder_mead(err, p0, [2, 0.05, 100, 100, 100, 20, 100, 0.05])
    p, v = nelder_mead(err, p, [0.5, 0.01, 20, 20, 20, 5, 20, 0.01])
    if best is None or v < best[1]: best = (p, v)
p, v = best
rms = math.sqrt(v/len(CP))
print("params", [round(x,6) for x in p]); print("rms px", round(rms,2))
res = sorted(((math.hypot(model(p,la,lo)[0]-x, model(p,la,lo)[1]-y), la, lo) for la,lo,x,y in CP), reverse=True)[:5]
print("worst residuals", [(round(r,1), la, lo) for r,la,lo in res])
print("PROJ = { lon0: %.6f, n: %.6f, a: %.6f, b: %.6f, c: %.6f, x0: %.6f, y0: %.6f, sy: %.6f }" % tuple(p))

