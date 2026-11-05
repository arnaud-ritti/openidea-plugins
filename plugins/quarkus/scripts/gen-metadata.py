#!/usr/bin/env python3
"""Regenerates data/quarkus-properties.tsv from the configuration model that
Quarkus extensions ship in their jars (Apache-2.0) and from the MicroProfile
metadata of Quarkus Tools for IntelliJ (EPL-2.0).

    python3 -I scripts/gen-metadata.py 3.40.1 data/quarkus-properties.tsv [extra-artifact-id…]

The jars are read in memory from Maven Central; nothing is extracted or run.

Output, one record per line, tab-separated:
    name  type  default  deprecation  description
deprecation is empty or `w:<replacement>`; hint lines start with `@`:
`@name  value  value…`.
"""

import io
import json
import re
import sys
import urllib.error
import urllib.request
import zipfile

# ponytail: curated core extensions, pass more artifact ids on the command line or switch to the io.quarkus BOM
EXTENSIONS = """quarkus-core quarkus-arc quarkus-vertx quarkus-vertx-http quarkus-rest quarkus-rest-jackson
quarkus-rest-client quarkus-jackson quarkus-hibernate-orm quarkus-hibernate-orm-panache quarkus-hibernate-validator
quarkus-datasource quarkus-agroal quarkus-jdbc-postgresql quarkus-jdbc-mysql quarkus-jdbc-h2
quarkus-reactive-pg-client quarkus-flyway quarkus-liquibase quarkus-smallrye-health quarkus-smallrye-openapi
quarkus-smallrye-fault-tolerance quarkus-smallrye-jwt quarkus-micrometer quarkus-opentelemetry quarkus-oidc
quarkus-oidc-client quarkus-security quarkus-messaging-kafka quarkus-kafka-client quarkus-scheduler quarkus-cache
quarkus-redis-client quarkus-mongodb-client quarkus-qute quarkus-mailer quarkus-grpc quarkus-websockets-next
quarkus-container-image-jib quarkus-kubernetes quarkus-logging-json quarkus-config-yaml""".split()
MODEL = "META-INF/quarkus-config-doc/quarkus-config-model.json"
JAVADOC = "META-INF/quarkus-config-doc/quarkus-config-javadoc.json"
MAVEN = "https://repo1.maven.org/maven2/io/quarkus"
STATIC = ("https://raw.githubusercontent.com/redhat-developer/intellij-quarkus/"
          "be058542ce55eda5a72d3b35a205d3027e8a906f/src/main/resources/static-properties/")
MICROPROFILE = ["mp-config", "mp-context-propagation", "mp-graphql", "mp-health", "mp-jwt", "mp-lra", "mp-metrics",
                "mp-openapi", "mp-opentracing"]
MAX_DESCRIPTION = 110
MAX_DEFAULT = 60


def clean(text):
    return re.sub(r"\s+", " ", str(text)).strip()


def simple_type(t):
    t = re.sub(r"\b[a-z][a-z0-9_]*\.", "", t or "")
    return t.replace("$", ".").replace(" ", "")


def first_sentence(text):
    text = re.split(r"<p>|<p/>|\n\s*@\w+|^\s*@\w+", text or "", maxsplit=1)[0]
    text = re.sub(r"\{@(?:code|link|linkplain|literal|value)\s+([^}]*)\}", r"\1", text)
    text = clean(re.sub(r"<[^>]+>", "", text))
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


def fetch(url):
    with urllib.request.urlopen(url) as r:
        return r.read()


def properties(items):
    for item in items:
        yield from properties(item.get("items", []))
        if item.get("@class", "").endswith("ConfigProperty"):
            yield item


def property_name(item):
    name = item["path"]["property"]
    if '"' not in name:
        return name
    head, quoted, rest = name.partition('."')
    if item.get("map") and quoted and rest.endswith('"') and '"' not in rest[:-1]:
        return head
    # ponytail: named-map entries need wildcard segments in spring_text::metadata
    return None


def extension_records(artifact, version, props, hints, levels):
    for jar_name in (artifact, f"{artifact}-deployment"):
        try:
            jar = zipfile.ZipFile(io.BytesIO(fetch(f"{MAVEN}/{jar_name}/{version}/{jar_name}-{version}.jar")))
        except urllib.error.HTTPError as e:
            if e.code != 404:
                raise
            print(f"skipped {jar_name}: not found", file=sys.stderr)
            continue
        names = set(jar.namelist())
        if MODEL not in names:
            continue
        elements = json.loads(jar.read(JAVADOC)).get("elements", {}) if JAVADOC in names else {}
        for root in json.loads(jar.read(MODEL)).get("configRoots", []):
            for item in properties(root.get("items", [])):
                name = property_name(item)
                if name is None or name in props:
                    continue
                doc = elements.get(f"{item.get('sourceType')}.{item.get('sourceElementName')}", {})
                ty = item.get("typeDescription") or simple_type(item.get("type"))
                deprecation = "w:" if "deprecation" in item else ""
                if "deprecation" in item:
                    deprecation += clean(item["deprecation"].get("replacement", ""))
                props[name] = [name, ty, default_value(item.get("defaultValue")), deprecation,
                               first_sentence(doc.get("description", ""))]
                values = item.get("enumAcceptedValues", {}).get("values", {})
                accepted = [clean(v["configValue"]) for v in values.values() if "configValue" in v]
                if accepted:
                    hints.setdefault(name, accepted)
                elif ty == "Level":
                    levels.append(name)


def microprofile(props, hints):
    for stem in MICROPROFILE:
        data = json.loads(fetch(f"{STATIC}{stem}-metadata.json"))
        for p in data.get("properties", []):
            if p["name"] in props:
                continue
            props[p["name"]] = [p["name"], simple_type(p.get("type")), default_value(p.get("defaultValue")), "",
                                first_sentence(p.get("description", ""))]
        for h in data.get("hints", []):
            values = [clean(v["value"]) for v in h.get("values", []) if "value" in v]
            if values:
                hints.setdefault(h["name"], values)


def main(version, out, extra):
    props, hints, levels = {}, {}, []
    for artifact in EXTENSIONS + extra:
        extension_records(artifact, version, props, hints, levels)
    microprofile(props, hints)
    log_levels = json.loads(fetch(f"{STATIC}jboss-logmanager-metadata.json"))
    level_values = [clean(v["value"]) for h in log_levels.get("hints", []) if h["name"] == "java.util.logging.Level"
                    for v in h["values"]]
    for name in levels:
        hints.setdefault(name, level_values)
    lines = ["\t".join(f.replace("\t", " ") for f in props[name]) for name in sorted(props)]
    lines += ["\t".join(["@" + name] + [v.replace("\t", " ") for v in hints[name]]) for name in sorted(hints)]
    with open(out, "w", encoding="utf-8") as f:
        f.write(f"# Quarkus {version} configuration metadata (Apache-2.0) and MicroProfile metadata from "
                "Quarkus Tools for IntelliJ 2.6.1 (EPL-2.0), see NOTICE\n")
        f.write("\n".join(lines) + "\n")
    print(f"{len(props)} properties, {len(hints)} hints")


if __name__ == "__main__":
    main(sys.argv[1], sys.argv[2], sys.argv[3:])
