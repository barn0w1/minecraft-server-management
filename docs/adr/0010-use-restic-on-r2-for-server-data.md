# ADR-0010: Use restic repositories on Cloudflare R2 for Server Data

Status: Superseded by ADR-0014

## Context

Nodeを交換可能にするため、Minecraft persistent filesをCloudflare R2上のrestic repositoryへ保存する方針を採用しました。

## Original decision

MinecraftServerごとにrepositoryを作り、empty restic passwordを使用し、backup後の追加verificationを設計対象としました。

## Supersession

[ADR-0014](0014-back-up-server-home-with-restic.md)はbackup単位を`Server Home`へ拡張し、restic exit code 0を成功contractとして信頼します。また、empty passwordではなくDeployment-wide shared passwordを使用します。
