import * as path from "node:path";
import * as cdk from "aws-cdk-lib";
import { Construct } from "constructs";
import * as ec2 from "aws-cdk-lib/aws-ec2";
import * as ecs from "aws-cdk-lib/aws-ecs";
import * as ecsPatterns from "aws-cdk-lib/aws-ecs-patterns";
import * as ecrAssets from "aws-cdk-lib/aws-ecr-assets";
import * as logs from "aws-cdk-lib/aws-logs";

export class CtxEcsStack extends cdk.Stack {
  constructor(scope: Construct, id: string, props?: cdk.StackProps) {
    super(scope, id, props);

    const serviceName = this.node.tryGetContext("serviceName") ?? "ctx-server";
    const desiredCount = Number(this.node.tryGetContext("desiredCount") ?? 1);
    const cpu = Number(this.node.tryGetContext("cpu") ?? 512);
    const memoryMiB = Number(this.node.tryGetContext("memoryMiB") ?? 1024);

    const vpc = new ec2.Vpc(this, "Vpc", {
      maxAzs: 2,
      natGateways: 0,
    });

    const cluster = new ecs.Cluster(this, "Cluster", {
      vpc,
      clusterName: `${serviceName}-cluster`,
    });

    // CDK builds this Dockerfile on deploy, pushes it to ECR assets, and
    // updates the ECS service task definition with the new image digest.
    const imageAsset = new ecrAssets.DockerImageAsset(this, "CtxServerImage", {
      directory: path.join(__dirname, "../..", ".."),
      file: "infra/docker/ecs.Dockerfile",
      platform: ecrAssets.Platform.LINUX_AMD64,
    });

    const logGroup = new logs.LogGroup(this, "CtxLogGroup", {
      retention: logs.RetentionDays.ONE_WEEK,
      removalPolicy: cdk.RemovalPolicy.DESTROY,
    });

    const taskDef = new ecs.FargateTaskDefinition(this, "TaskDef", {
      cpu,
      memoryLimitMiB: memoryMiB,
    });

    taskDef.addContainer("CtxServer", {
      image: ecs.ContainerImage.fromDockerImageAsset(imageAsset),
      logging: ecs.LogDrivers.awsLogs({
        streamPrefix: "ctx-server",
        logGroup,
      }),
      environment: {
        CTX_HOST: "0.0.0.0",
        CTX_PORT: "8080",
        PORT: "8080",
        CTX_PATH: "/mnt/ctx-contexts",
      },
      portMappings: [{ containerPort: 8080 }],
    });

    const service = new ecsPatterns.ApplicationLoadBalancedFargateService(
      this,
      "Service",
      {
        cluster,
        serviceName,
        taskDefinition: taskDef,
        desiredCount,
        publicLoadBalancer: true,
        assignPublicIp: true,
      }
    );

    service.targetGroup.configureHealthCheck({
      path: "/status",
      healthyHttpCodes: "200",
      interval: cdk.Duration.seconds(30),
    });

    new cdk.CfnOutput(this, "LoadBalancerDNS", {
      value: service.loadBalancer.loadBalancerDnsName,
    });
  }
}
