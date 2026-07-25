# ADR-0004: Treat external mutation timeouts as uncertain

Status: Superseded by ADR-0013

## Context

network response lossはexternal operationの非実行を意味せず、blind retryはduplicate effectを起こし得ます。

## Original decision

mutation response lossをuncertainとして分類し、read-only observationで確定できない場合はIncidentへ移行する方針を採用しました。

## Supersession

この判断はuncertaintyを正しく認識しましたが、通常failureまでIncidentへ近づけ、operation-specific recoveryを弱くしました。

[ADR-0013](0013-use-durable-operations-and-idempotent-agent-commands.md)は、Unknown Outcomeをdurable Operation、idempotent replay、observationで自動収束させ、Incidentをunsafe contradictionに限定します。
