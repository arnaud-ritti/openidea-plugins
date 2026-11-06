# Changelog

## [0.2.0](https://github.com/arnaud-ritti/openidea-plugins/compare/kube-explorer-v0.1.0...kube-explorer-v0.2.0) (2026-11-06)


### Features

* **kube-explorer:** browse all namespaces ([ff6cec1](https://github.com/arnaud-ritti/openidea-plugins/commit/ff6cec1bff4e58227d5e37c69105c69e34e4a82c))
* **kube-explorer:** convert a List to separate documents and documents to a List ([2f2a7f0](https://github.com/arnaud-ritti/openidea-plugins/commit/2f2a7f06c930bc5149e5f0d03138fa1ebd990677))
* **kube-explorer:** debug a pod with an ephemeral container ([75f0406](https://github.com/arnaud-ritti/openidea-plugins/commit/75f0406967662c09a852e49ed49a38d97bb10f75))
* **kube-explorer:** deprecated API table, Chart.yaml schema and excessive trim checks ([cfc6133](https://github.com/arnaud-ritti/openidea-plugins/commit/cfc6133d6c2862f4ede5fc09f3a0925089482961))
* **kube-explorer:** inspect manifests for removed apiVersions, duplicate env names, missing names and selectors, Helm and Chart.yaml mistakes ([9da6db5](https://github.com/arnaud-ritti/openidea-plugins/commit/9da6db57dabafbedd4c6530caaeb063cee8f2988))
* **kube-explorer:** list custom resources by API group ([4445764](https://github.com/arnaud-ritti/openidea-plugins/commit/4445764abb0f901ec80892f53b59f8523d5725cb))
* **kube-explorer:** New-menu templates for the common Kubernetes resources ([c9635b4](https://github.com/arnaud-ritti/openidea-plugins/commit/c9635b461814e02687af94001d5f8ceabd6f8189))
* **kube-explorer:** show a secret decoded in the panel, create a secret over stdin ([ab451dd](https://github.com/arnaud-ritti/openidea-plugins/commit/ab451dd83c12693bd887328816d687c05d352132))
* **kube-explorer:** update Helm repositories, kubectl install hint, CSR template ([98d03b6](https://github.com/arnaud-ritti/openidea-plugins/commit/98d03b65e7dd4840d28df823cd6b12c4dccc2983))


### Bug Fixes

* **kube-explorer:** correct the extensions PodSecurityPolicy release, quiet on patches, cap file size ([726288e](https://github.com/arnaud-ritti/openidea-plugins/commit/726288e624617edd3acd3d5c0bd91d9e5bfa4e08))
* **kube-explorer:** exclude every built-in API group from custom resources, one api-resources call ([ae11d8a](https://github.com/arnaud-ritti/openidea-plugins/commit/ae11d8adee0ace1a0bf703e9d486abdb70422e0c))
* **kube-explorer:** gate secret YAML, create without overwriting, harden debug names and JSON scan ([d48948a](https://github.com/arnaud-ritti/openidea-plugins/commit/d48948a88e5610e9a431911bd2b2badf9289e213))
* **kube-explorer:** keep Chart.yaml offsets on char boundaries, refuse manifest writes in all namespaces ([97a3a00](https://github.com/arnaud-ritti/openidea-plugins/commit/97a3a00bc38dcf2cc6d04a404a5484ebb4b95e77))
* **kube-explorer:** offer the kubectl install hint only when kubectl cannot start ([daaf35d](https://github.com/arnaud-ritti/openidea-plugins/commit/daaf35dc74feb1e87727d3bd280d3edeed512742))
* **kube-explorer:** treat storagemigration.k8s.io as built in, test empty output ([af22344](https://github.com/arnaud-ritti/openidea-plugins/commit/af223443e878affa36e2cf67825b2704318358f2))
