#!/usr/bin/env python3
"""Regenerates data/boot-properties.tsv from the configuration metadata that
Spring Boot ships in its jars (Apache-2.0).

    python3 -I scripts/gen-metadata.py 3.5.16 data/boot-properties.tsv 2025.0.3

The jars are read in memory from Maven Central; nothing is extracted. The third
argument is the Spring Cloud release train (Apache-2.0): the artifact versions
come from its spring-cloud-dependencies BOM, and artifacts without published
metadata are reported on stderr and skipped. Boot's keys win over Cloud's.

Output, one record per line, tab-separated:
    name  type  default  deprecation  description
deprecation is empty, `w:<replacement>` (deprecated) or `e:<replacement>`
(no longer supported); hint lines start with `@`: `@name  value  value…`.
"""

import io
import json
import re
import sys
import urllib.error
import urllib.request
import zipfile

ARTIFACTS = ["spring-boot", "spring-boot-autoconfigure", "spring-boot-actuator-autoconfigure"]
ENTRIES = ["META-INF/spring-configuration-metadata.json", "META-INF/additional-spring-configuration-metadata.json"]
CLOUD_BASE = "https://repo1.maven.org/maven2/org/springframework/cloud"
# BOM version property (without `.version`) -> artifacts.
CLOUD = {
    "spring-cloud-commons": ["spring-cloud-commons", "spring-cloud-context", "spring-cloud-loadbalancer"],
    "spring-cloud-config": ["spring-cloud-config-client", "spring-cloud-config-server"],
    "spring-cloud-netflix": ["spring-cloud-netflix-eureka-client", "spring-cloud-netflix-eureka-server"],
    "spring-cloud-gateway": ["spring-cloud-gateway-server", "spring-cloud-gateway-server-webflux",
                             "spring-cloud-gateway-server-webmvc", "spring-cloud-gateway-proxyexchange-webflux",
                             "spring-cloud-gateway-proxyexchange-webmvc"],
    "spring-cloud-openfeign": ["spring-cloud-openfeign-core"],
    "spring-cloud-stream": ["spring-cloud-stream", "spring-cloud-stream-binder-kafka",
                            "spring-cloud-stream-binder-kafka-streams", "spring-cloud-stream-binder-rabbit"],
    "spring-cloud-circuitbreaker": ["spring-cloud-circuitbreaker-resilience4j",
                                    "spring-cloud-circuitbreaker-spring-retry"],
    "spring-cloud-bus": ["spring-cloud-bus"],
    "spring-cloud-consul": ["spring-cloud-consul-core", "spring-cloud-consul-discovery", "spring-cloud-consul-config"],
    "spring-cloud-zookeeper": ["spring-cloud-zookeeper-core", "spring-cloud-zookeeper-discovery",
                               "spring-cloud-zookeeper-config"],
    "spring-cloud-kubernetes": ["spring-cloud-kubernetes-commons", "spring-cloud-kubernetes-client-config",
                                "spring-cloud-kubernetes-client-discovery", "spring-cloud-kubernetes-fabric8-config",
                                "spring-cloud-kubernetes-fabric8-discovery"],
    "spring-cloud-vault": ["spring-cloud-vault-config"],
    "spring-cloud-contract": ["spring-cloud-contract-verifier", "spring-cloud-contract-stub-runner"],
    "spring-cloud-task": ["spring-cloud-task-core"],
    "spring-cloud-function": ["spring-cloud-function-context", "spring-cloud-function-web"],
}
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


def cloud_jars(train):
    url = f"{CLOUD_BASE}/spring-cloud-dependencies/{train}/spring-cloud-dependencies-{train}.pom"
    with urllib.request.urlopen(url, timeout=60) as r:
        pom = r.read().decode()
    for prop, artifacts in CLOUD.items():
        m = re.search(rf"<{prop}\.version>([^<]+)</", pom)
        if not m:
            print(f"skipped {prop}: not in {train}", file=sys.stderr)
            continue
        for artifact in artifacts:
            yield artifact, f"{CLOUD_BASE}/{artifact}/{m.group(1)}/{artifact}-{m.group(1)}.jar"


def jars(version, train):
    for artifact in ARTIFACTS:
        yield artifact, f"https://repo1.maven.org/maven2/org/springframework/boot/{artifact}/{version}/{artifact}-{version}.jar"
    if train:
        yield from cloud_jars(train)


def main(version, out, train=None):
    props, hints = {}, {}
    for artifact, url in jars(version, train):
        try:
            with urllib.request.urlopen(url, timeout=60) as r:
                jar = zipfile.ZipFile(io.BytesIO(r.read()))
        except urllib.error.HTTPError as e:
            if e.code != 404 or artifact in ARTIFACTS:
                raise
            print(f"skipped {artifact}: not found", file=sys.stderr)
            continue
        before = len(props)
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
        if len(props) == before and artifact not in ARTIFACTS:
            print(f"skipped {artifact}: no new configuration properties", file=sys.stderr)
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
        cloud = f" and Spring Cloud {train}" if train else ""
        f.write(f"# Spring Boot {version}{cloud} configuration metadata (Apache-2.0), see NOTICE\n")
        f.write("\n".join(lines) + "\n")


if __name__ == "__main__":
    main(*sys.argv[1:4])
