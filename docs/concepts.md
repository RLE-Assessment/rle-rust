---
title: Understanding the criteria
subtitle: What the metrics measure, how the thresholds combine, and how to read a range
---

This page contains no code. It explains what the library computes and why the answers take
the shape they do. Everything here follows
[IUCN (2024), *Guidelines for the application of IUCN Red List of Ecosystems Categories and
Criteria*, Version 2.0](https://portals.iucn.org/library/node/51533); section and page
references point into that document.

## The eight categories

An assessment places an ecosystem type in one of eight categories. Four of them —
Collapsed, Critically Endangered, Endangered and Vulnerable — count as **threatened**.

| | Category | Meaning |
|---|---|---|
| **CO** | Collapsed | The ecosystem has lost its defining features throughout its distribution |
| **CR** | Critically Endangered | Extremely high risk of collapse |
| **EN** | Endangered | Very high risk of collapse |
| **VU** | Vulnerable | High risk of collapse |
| **NT** | Near Threatened | Close to qualifying as threatened |
| **LC** | Least Concern | Assessed against the criteria and does not qualify |
| **DD** | Data Deficient | Not enough information to assess |
| **NE** | Not Evaluated | Not yet assessed against the criteria |

Two of these are easy to conflate and mean very different things. **LC is a finding** — the
ecosystem was assessed and does not qualify as threatened. **DD and NE are the absence of a
finding.** DD means the assessment was attempted and the evidence was insufficient; NE means
it was never attempted. The library keeps all three distinct, and never reports LC where it
means "we did not check".

An ecosystem is listed under the **most threatened** category that any single criterion
supports. A finding of CR under one criterion is not softened by LC under another.

## Criterion B: restricted geographic distribution

Criterion B asks whether an ecosystem's distribution is small enough that a single
catastrophe or a few threatening events could plausibly collapse it. It has three
sub-criteria, and meeting **any one** of them is sufficient.

### B1 — extent of occurrence (EOO)

The area of the smallest convex polygon enclosing every known occurrence, in km².

The critical and frequently-misapplied rule (§6.3.2, p. 67): this polygon **must not
exclude anything**. Oceans within the range of a terrestrial ecosystem stay in. Land within
the range of a marine one stays in. Areas outside your study area stay in. Clipping the
hull to the ecosystem's actual habitat produces a smaller EOO and therefore an inflated
threat category, and it makes the number incomparable with every other assessment.

EOO measures how spread out the risk is, not how much ecosystem there is.

### B2 — area of occupancy (AOO)

The number of occupied 10 × 10 km grid cells.

The grid size is fixed by the Guidelines rather than chosen per assessment, because extent
estimates are extremely sensitive to the resolution of the source map. A standard grain
makes assessments comparable. The Guidelines note four reasons for choosing cells this
large, including that ecosystem boundaries are inherently vague and that larger cells
better predict real-world risk.

**AOO is a count of cells, not an area.** An AOO of 20 means twenty occupied cells, which
is why the thresholds are small integers.

#### The 1% exclusion

Many ecosystems have a few large patches and a long tail of tiny ones. Those tiny patches
contribute almost nothing to spreading risk, but each one still occupies a whole grid cell,
which would overstate the ecosystem's resilience. So the count is corrected (§6.3.2, p. 67):

1. Intersect the AOO grid with the distribution map.
2. Compute the ecosystem's extent in each cell, and sum them to get the total.
3. Sort cells ascending by that extent, smallest first.
4. Take the running cumulative sum.
5. **Count the cells whose cumulative proportion exceeds 0.01** — that is, drop the smallest
   cells that together account for up to 1% of the mapped extent.

This replaces an earlier rule from Guidelines v1.1, which excluded cells where the ecosystem
covered less than 1 km². That rule could exclude *every* cell when patches were small and
widely separated. If you are comparing against an older tool, check which correction it
applies.

### B3 — very few threat-defined locations

For ecosystems where the spatial data is too sparse to estimate EOO or AOO, but where a
small number of plausible events could clearly cause collapse.

B3 has two limbs and **both** must hold: very few threat-defined locations (generally fewer
than five), **and** the ecosystem being prone enough to human activity or stochastic events
that it could collapse or become Critically Endangered within a very short period —
generally the next two decades.

**B3 can only ever produce Vulnerable** (§6.3.3, p. 75). It compensates for weaker,
qualitative evidence by capping how much threat it can assert.

### Threat-defined locations

A threat-defined location is *not* a site or a patch. It is a geographically or ecologically
distinct area in which **a single threatening event could rapidly affect every occurrence**.
Its size is determined entirely by the spatial footprint of the most serious plausible
threat, so the same ecosystem can occupy a different number of locations depending on which
threat you are considering.

Individual logged patches in one concession under one regulatory regime are one location,
not many, because the same market and legal forces drive them together.

## The sub-condition gate

This is the part most often lost when assessments are automated.

**Meeting a spatial threshold does not produce a listing.** B1 and B2 each require the
threshold **and** at least one of three clauses:

- **(a)** an observed or inferred continuing decline in *any of* (i) spatial extent,
  (ii) environmental quality, or (iii) biotic interactions;
- **(b)** observed or inferred threatening processes likely to cause continuing declines
  within the next 20 years;
- **(c)** the ecosystem exists at few threat-defined locations.

An ecosystem with a tiny distribution that is stable, unthreatened and not declining does
**not** qualify under Criterion B. That is the intended answer, not a gap.

### Clause (c) is a count, and it depends on the category

Clauses (a) and (b) are yes-or-no. Clause (c) is neither — it is a threshold on the number
of threat-defined locations, and **the threshold is different for each category**:

| Category | Clause (c) is met at |
|---|---|
| CR | exactly 1 threat-defined location |
| EN | 5 or fewer |
| VU | 10 or fewer |

This has a consequence that is easy to get wrong. Consider an ecosystem with an EOO of
1,500 km² — comfortably inside the CR band — three threat-defined locations, and no
evidence of decline or threatening processes:

- **CR** requires the EOO threshold *and* exactly one location. Three locations fails.
- **EN** requires its EOO threshold *and* five or fewer locations. Three passes.

The correct listing is **EN**, not CR. Evaluating the spatial threshold first and then
applying clause (c) as a simple yes/no gate gives CR, and overstates the threat by a full
category. The library evaluates each category level independently for exactly this reason.

## Reading a category range

An outcome like `EN (LC–EN)` reads as: **best estimate EN, plausibly anywhere from LC to
EN.** The bounds are ordered least-threatened first.

This is not a softening of the result. The Guidelines require it (§6.3.2, p. 70):

> In cases where continuing declines are equally likely to be occurring or not occurring,
> upper and lower bounds of the status under criterion B should be estimated by propagating
> both scenarios through the criteria.

Ranges arise from two independent sources, and they compose:

**Uncertainty in the metric.** An EOO of 20,000 km² that is plausibly 15,000–25,000 straddles
the EN/VU boundary, giving `EN (VU–EN)`.

**Uncertainty in the sub-conditions.** Metrics in the EN band with no clause established
gives `EN (LC–EN)` — because if a clause turns out to be met the answer is EN, and if all
three are eventually ruled out the answer is LC.

A single category with no parentheses means the outcome is settled.

### What each piece of evidence changes

| What you record | Effect |
|---|---|
| A clause is **met** | The threshold category stands, firmly |
| All three clauses **assessed and not met** | Criterion B is not triggered: **LC** |
| Clauses **not assessed** | A range from LC to the threshold category |
| **No plausible threats exist** | Clause (c) and B3 are *not met* — a finding |
| **Locations cannot be assessed** | **DD** for that sub-criterion |

The last two are deliberately different, following Box 13 step 5. "There are no threats" is
a conclusion. "We could not determine the threats" is an absence of one. Recording the first
where you mean the second turns ignorance into false reassurance.

## Near Threatened

No threshold in this library produces NT, because NT has no numeric breakpoint in the
criteria — it is a judgement call about being close to qualifying.

It is not unreachable, though. Box 13 step 6(iv) describes a conditional NT pathway for
B1(c), B2(c) and B3 that applies when information is insufficient *and* less than 30% of the
distribution is unthreatened. Those conditions depend on judgements the library cannot make,
so when a location count would fall in range the library attaches a note pointing you at the
rule rather than deciding for you.

## Provenance

Every result carries the Guidelines edition applied, the engine version, and a SHA-256
digest of the threshold table. Thresholds change between editions of the criteria, so a
category is only reproducible if you know which numbers produced it.

Note that two version numbers are in play and both matter: these are the **Guidelines
version 2.0** (2024), which codify the **Criteria version 2.1**.

## Where this leaves you

The library will tell you what the criteria imply about the numbers you give it. It will not
tell you whether your distribution map is any good, whether your collapse threshold is
defensible, or whether you have identified the right threats. Those judgements are the
assessment, and they remain yours.
