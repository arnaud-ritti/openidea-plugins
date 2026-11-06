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

# Curated by popularity; ids without published config metadata are reported on stderr and skipped.
# ponytail: extend the list or pass more artifact ids on the command line; the io.quarkus BOM would need a pom parser
EXTENSIONS = """quarkus-core quarkus-arc quarkus-vertx quarkus-vertx-http quarkus-vertx-graphql quarkus-reactive-routes
quarkus-undertow quarkus-web-dependency-locator quarkus-info quarkus-tls-registry
quarkus-rest quarkus-rest-jackson quarkus-rest-jsonb quarkus-rest-jaxb quarkus-rest-client quarkus-rest-client-jackson
quarkus-rest-client-oidc-filter quarkus-rest-client-oidc-token-propagation quarkus-rest-links quarkus-rest-qute
quarkus-resteasy quarkus-resteasy-jackson quarkus-resteasy-client quarkus-websockets quarkus-websockets-next
quarkus-jackson quarkus-jsonb quarkus-jaxb quarkus-grpc quarkus-smallrye-graphql quarkus-smallrye-graphql-client
quarkus-hibernate-orm quarkus-hibernate-orm-panache quarkus-hibernate-orm-rest-data-panache quarkus-hibernate-envers
quarkus-hibernate-reactive quarkus-hibernate-reactive-panache quarkus-hibernate-search-orm-elasticsearch
quarkus-hibernate-validator quarkus-narayana-jta quarkus-narayana-lra
quarkus-datasource quarkus-agroal quarkus-jdbc-postgresql quarkus-jdbc-mysql quarkus-jdbc-mariadb quarkus-jdbc-mssql
quarkus-jdbc-oracle quarkus-jdbc-db2 quarkus-jdbc-derby quarkus-jdbc-h2 quarkus-reactive-datasource
quarkus-reactive-pg-client quarkus-reactive-mysql-client quarkus-reactive-mssql-client quarkus-reactive-oracle-client
quarkus-reactive-db2-client quarkus-flyway quarkus-liquibase quarkus-liquibase-mongodb
quarkus-mongodb-client quarkus-mongodb-panache quarkus-redis-client quarkus-infinispan-client quarkus-neo4j
quarkus-elasticsearch-rest-client quarkus-elasticsearch-java-client quarkus-cache quarkus-spring-data-jpa
quarkus-smallrye-health quarkus-smallrye-openapi quarkus-smallrye-fault-tolerance quarkus-smallrye-jwt
quarkus-smallrye-jwt-build quarkus-smallrye-context-propagation quarkus-smallrye-stork quarkus-smallrye-opentracing
quarkus-smallrye-reactive-messaging quarkus-micrometer quarkus-micrometer-registry-prometheus quarkus-opentelemetry
quarkus-opentelemetry-exporter-otlp quarkus-observability-devservices
quarkus-oidc quarkus-oidc-client quarkus-oidc-client-filter quarkus-oidc-token-propagation quarkus-oidc-db-token-state-manager
quarkus-oidc-redis-token-state-manager quarkus-keycloak-authorization quarkus-security quarkus-security-jpa
quarkus-security-ldap quarkus-security-webauthn quarkus-elytron-security-properties-file quarkus-elytron-security-jdbc
quarkus-elytron-security-oauth2
quarkus-messaging-kafka quarkus-kafka-client quarkus-kafka-streams quarkus-messaging-amqp quarkus-messaging-rabbitmq
quarkus-messaging-pulsar quarkus-scheduler quarkus-quartz quarkus-mailer quarkus-qute
quarkus-funqy-http quarkus-funqy-knative-events quarkus-amazon-lambda quarkus-azure-functions-http
quarkus-google-cloud-functions quarkus-container-image-jib quarkus-container-image-docker
quarkus-container-image-buildpack quarkus-container-image-openshift quarkus-container-image-podman
quarkus-kubernetes quarkus-kubernetes-client quarkus-kubernetes-config quarkus-kubernetes-service-binding
quarkus-openshift quarkus-knative quarkus-minikube quarkus-kind quarkus-helm
quarkus-logging-json quarkus-logging-gelf quarkus-logging-sentry quarkus-config-yaml quarkus-picocli quarkus-jacoco
quarkus-cyclonedx""".split()
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
    with urllib.request.urlopen(url, timeout=60) as r:
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
    before = len(props)
    read_extension(artifact, version, props, hints, levels)
    if len(props) == before:
        print(f"skipped {artifact}: no new configuration properties", file=sys.stderr)


def read_extension(artifact, version, props, hints, levels):
    for jar_name in (artifact, f"{artifact}-deployment"):
        try:
            jar = zipfile.ZipFile(io.BytesIO(fetch(f"{MAVEN}/{jar_name}/{version}/{jar_name}-{version}.jar")))
        except urllib.error.HTTPError as e:
            if e.code != 404:
                raise
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
    for artifact in dict.fromkeys(EXTENSIONS + extra):
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
