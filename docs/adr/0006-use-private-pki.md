# ADR-0006: Use a private PKI with an offline Root CA

Status: Superseded by ADR-0012

## Context

Node client identityをcertificateで表現し、Control Plane serverとAgentをmutual TLSで認証するためprivate PKIを選択しました。

## Original decision

Offline Root CA、separate issuing intermediates、short-lived Agent certificatesをv1 foundationとしました。

## Supersession

small-community向けv1としてCA custody、issuer、rotation、revocationの実装負担が大きく、automation価値へ直接つながりませんでした。

[ADR-0012](0012-use-json-rpc-over-http2-agent-pull.md)はserver TLSとper-Node bearer credentialをv1 contractとし、mTLSをfuture hardeningへ移します。
