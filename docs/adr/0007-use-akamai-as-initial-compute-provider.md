# ADR-0007: Use Akamai Cloud as the initial compute provider

Status: Accepted

## Context

実際のdeployment targetはAkamaiであり、provider-neutral modelを先に作ると、最初のproviderの重要なidentity、status、error、ownership semanticsを隠す。

## Decision

Node provisioningのinitial providerとしてAkamai Cloud/Linode APIを直接targetにする。generic multi-provider plugin systemを作らない。

## Consequences

Node domainとAkamai adapterの責務は分離するが、second providerが実際に必要になるまでgeneric abstractionを約束しない。

## Related documents

- [Design principles](../design-principles.md)
- [Architecture overview](../architecture/overview.md)
