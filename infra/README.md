# infra

Infrastructure-as-code for deploying `ctx-server` to AWS ECS Fargate using CDK.

## What this deploys

- a VPC
- an ECS cluster
- a public Application Load Balancer
- a Fargate service running `ctx-server`
- a Docker image asset built from this repository and pushed by CDK

The stack image source is `infra/docker/ecs.Dockerfile`.

## First-time manual setup (one-time)

1. **Install prerequisites**
   - AWS CLI configured for target account/region
   - Bun
   - CDK CLI (installed through `infra/package.json`)

2. **Install infra deps**

   ```bash
   cd infra
   bun install
   ```

3. **Bootstrap CDK in your account/region**

   ```bash
   bunx cdk bootstrap
   ```

   If you previously hit `ENAMETOOLONG` during synth/deploy, remove the old output and retry:

   ```bash
   rm -rf cdk.out
   ```

4. **Deploy once manually**

   ```bash
   bunx cdk deploy --require-approval never
   ```

5. **Capture output**
   - Note the `LoadBalancerDNS` stack output.
   - Validate health:

   ```bash
   curl "http://<LoadBalancerDNS>/status"
   ```

## GitHub Actions continuous deploy

The workflow `.github/workflows/deploy.yml` deploys automatically on push to `main`.

Set these repository secrets/variables:

- **Secret:** `AWS_ROLE_TO_ASSUME` (OIDC role ARN for GitHub Actions)
- **Variable:** `AWS_REGION` (for example `us-east-1`)

The workflow runs `bunx cdk deploy --require-approval never`, which rebuilds/pushes the Docker image and rolls the ECS service.

## Optional CDK context overrides

```bash
bunx cdk deploy --require-approval never \
  -c serviceName=ctx-server \
  -c desiredCount=1 \
  -c cpu=512 \
  -c memoryMiB=1024
```

## Persistence note

This baseline uses Fargate ephemeral storage for `CTX_PATH` (`/mnt/ctx-contexts`).
For persistent contexts across task replacement, add EFS and mount it at `CTX_PATH`.
