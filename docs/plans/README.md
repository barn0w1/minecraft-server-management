# Implementation plans

planは実装順序、scope、acceptance criteriaを定義します。architectureの正本ではありません。

## Sequence

0. [Architecture Reset](architecture-reset.md) — Completed
1. [Local Node v1](local-node-v1.md) — Proposed
2. Durable Operations and Reconnect
3. Backup and Restore
4. Akamai Node Lifecycle
5. Smart Automation
6. Hardening

後続milestoneの詳細planは、直前milestoneの実測と学習を反映して作成します。空のfuture planを大量に作りません。

## Milestone intent

### Milestone 0: Architecture Reset

resource、protocol、error model、runtime、backup、implementation sequenceをMinecraft automationへ集中させます。

### Milestone 1: Local Node v1

手動登録したreal GNU/Linux Node上で、Control Planeからitzg/minecraft-serverをstart、observe、stopできるend-to-end vertical sliceを作ります。

### Milestone 2: Durable Operations and Reconnect

SQLite Operation、Agent journal、at-least-once delivery、retry、restart recovery、Fencing Tokenを完成させます。

### Milestone 3: Backup and Restore

Server Home、restic/R2、online/offline backup、restore、Snapshot metadata、Node release gateを追加します。

### Milestone 4: Akamai Node Lifecycle

Akamai provisioning、bootstrap、enrollment、ownership、deleteをMinecraftServer lifecycleへ接続します。

### Milestone 5: Smart Automation

schedule、idle shutdown、update、notification、Discord Botを追加します。

### Milestone 6: Hardening

fault test、secret delivery改善、database recovery、optional mTLS評価、bounded real acceptanceを行います。

## Plan template

- Goal
- In scope
- Out of scope
- Required decisions
- Implementation slices
- Acceptance criteria
- Exit conditions
- Risks and open questions
