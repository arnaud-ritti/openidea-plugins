#!/usr/bin/env python3
"""Regenerates data/boot-properties.tsv from the configuration metadata that
Spring Boot ships in its jars (Apache-2.0).

    python3 -I scripts/gen-metadata.py 3.5.16 data/boot-properties.tsv

The jars are read in memory from Maven Central; nothing is extracted.

Output, one record per line, tab-separated:
    name  type  default  deprecation  description
deprecation is empty, `w:<replacement>` (deprecated) or `e:<replacement>`
(no longer supported); hint lines start with `@`: `@name  value  value…`.
"""

import io
import json
import re
import sys
import urllib.request
import zipfile

ARTIFACTS = ["spring-boot", "spring-boot-autoconfigure", "spring-boot-actuator-autoconfigure"]
ENTRIES = ["META-INF/spring-configuration-metadata.json", "META-INF/additional-spring-configuration-metadata.json"]
MAX_DESCRIPTION = 110
MAX_DEFAULT = 60


def clean(text):
    return re.sub(r"\s+", " ", str(text)).strip()


def simple_type(t):
    t = re.sub(r"\b[a-z][a-z0-9_]*\.", "", t or "")
    return t.replace("$", ".").replace(" ", "")


def first_sentence(text):
    text = clean(text)
    m = re.search(r"\.(\s|$)", text)
    if m:
        text = text[: m.start()]
    if len(text) > MAX_DESCRIPTION:
        text = text[: MAX_DESCRIPTION - 1].rstrip() + "…"
    return text


def default_value(v):
    if v is None:
        return ""
    if isinstance(v, bool):
        return "true" if v else "false"
    if isinstance(v, list):
        v = ",".join(default_value(x) for x in v)
    v = clean(v)
    return v if len(v) <= MAX_DEFAULT else v[: MAX_DEFAULT - 1] + "…"


def main(version, out):
    props, hints = {}, {}
    for artifact in ARTIFACTS:
        url = f"https://repo1.maven.org/maven2/org/springframework/boot/{artifact}/{version}/{artifact}-{version}.jar"
        with urllib.request.urlopen(url) as r:
            jar = zipfile.ZipFile(io.BytesIO(r.read()))
        names = set(jar.namelist())
        # The main file already merges the additional one: it wins.
        for entry in [e for e in ENTRIES if e in names]:
            data = json.loads(jar.read(entry))
            for p in data.get("properties", []):
                props.setdefault(p["name"], p)
            for h in data.get("hints", []):
                values = [clean(v["value"]) for v in h.get("values", []) if "value" in v]
                if values:
                    hints.setdefault(h["name"], values)
    lines = []
    for name in sorted(props):
        p = props[name]
        dep = p.get("deprecation")
        if dep is None and p.get("deprecated"):
            dep = {}
        deprecation = ""
        if dep is not None:
            level = "e" if dep.get("level") == "error" else "w"
            deprecation = f"{level}:{clean(dep.get('replacement', ''))}"
        fields = [name, simple_type(p.get("type")), default_value(p.get("defaultValue")), deprecation,
                  first_sentence(p.get("description", ""))]
        lines.append("\t".join(f.replace("\t", " ") for f in fields))
    for name in sorted(hints):
        lines.append("\t".join(["@" + name] + [v.replace("\t", " ") for v in hints[name]]))
    with open(out, "w", encoding="utf-8") as f:
        f.write(f"# Spring Boot {version} configuration metadata (Apache-2.0), see NOTICE\n")
        f.write("\n".join(lines) + "\n")


if __name__ == "__main__":
    main(sys.argv[1], sys.argv[2])
