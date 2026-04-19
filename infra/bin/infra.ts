#!/usr/bin/env node
import "source-map-support/register";
import * as cdk from "aws-cdk-lib";
import { CtxEcsStack } from "../lib/ctx-ecs-stack";

const app = new cdk.App();

new CtxEcsStack(app, "CtxEcsStack", {
  env: {
    account: process.env.CDK_DEFAULT_ACCOUNT,
    region: process.env.CDK_DEFAULT_REGION,
  },
});
