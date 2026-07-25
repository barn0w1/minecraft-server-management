# ADR-0005: Use QUIC, mTLS, and JSON-RPC for the Agent Protocol

Status: Superseded by ADR-0012

## Context

outbound long-lived connection上でbidirectional RPCを多重化するため、raw QUIC、mTLS、JSON-RPCを選択しました。

## Original decision

一request/responseを一QUIC streamへmappingし、custom length framingを使用するprotocolを採用しました。

## Supersession

raw QUICはframing、stream lifecycle、limit、debugging、PKIをproject固有に実装する範囲を増やしました。

[ADR-0012](0012-use-json-rpc-over-http2-agent-pull.md)はJSON-RPCを維持しつつ、HTTP/2、standard HTTPS、Agent-initiated pullへ置き換えます。
