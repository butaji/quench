// ink-static-block example — demonstrates class static blocks.
//
// ES2022 introduced static blocks in classes, which run once when the
// class is first evaluated. They're useful for complex static initialization.
//
// This example shows static blocks in action.

import React from 'react';
import { Box, Text } from 'ink';

// Track static block execution
const staticLog: string[] = [];

class DatabaseConfig {
  static host: string;
  static port: number;
  static url: string;
  static initialized: boolean = false;

  static {
    staticLog.push('DatabaseConfig static block');
    this.host = 'localhost';
    this.port = 5432;
    this.url = `postgresql://${this.host}:${this.port}`;
    this.initialized = true;
  }

  static getConnectionString(): string {
    return this.url;
  }
}

class ApiConfig {
  static baseUrl: string;
  static timeout: number;
  static apiKey: string;
  static headers: Record<string, string>;

  static {
    staticLog.push('ApiConfig static block');
    this.baseUrl = 'https://api.example.com';
    this.timeout = 30000;
    this.apiKey = 'demo-key-123';
    this.headers = {
      'Content-Type': 'application/json',
      'Authorization': `Bearer ${this.apiKey}`,
    };
  }

  static getHeaders(): Record<string, string> {
    return this.headers;
  }
}

class ServiceRegistry {
  static services: Map<string, string>;
  static instanceCount: number;

  static {
    staticLog.push('ServiceRegistry static block');
    this.services = new Map();
    this.services.set('db', 'DatabaseConfig');
    this.services.set('api', 'ApiConfig');
    this.instanceCount = 0;
  }

  static register(name: string): void {
    this.services.set(name, name);
    this.instanceCount++;
  }

  static getServiceCount(): number {
    return this.services.size;
  }
}

// Trigger the static blocks by accessing static members
const dbUrl = DatabaseConfig.getConnectionString();
const apiHeaders = ApiConfig.getHeaders();
const serviceCount = ServiceRegistry.getServiceCount();

// Register some services
ServiceRegistry.register('cache');
ServiceRegistry.register('queue');

export default function StaticBlockDemo() {
  return (
    <Box flexDirection="column" padding={1}>
      <Text bold color="cyan">Class Static Blocks Demo</Text>
      <Text dimColor>ES2022 static initialization</Text>
      <Text></Text>
      <Text>--- Static block execution ---</Text>
      {staticLog.map((log, i) => (
        <Text key={i}>{i + 1}. {log}</Text>
      ))}
      <Text></Text>
      <Text>--- DatabaseConfig ---</Text>
      <Text>host: {DatabaseConfig.host}</Text>
      <Text>port: {DatabaseConfig.port}</Text>
      <Text>url: {dbUrl}</Text>
      <Text>initialized: {DatabaseConfig.initialized ? 'true' : 'false'}</Text>
      <Text></Text>
      <Text>--- ApiConfig ---</Text>
      <Text>baseUrl: {ApiConfig.baseUrl}</Text>
      <Text>timeout: {ApiConfig.timeout}</Text>
      <Text>has Content-Type: {apiHeaders['Content-Type'] ? 'true' : 'false'}</Text>
      <Text></Text>
      <Text>--- ServiceRegistry ---</Text>
      <Text>services: {serviceCount}</Text>
      <Text>registered: {ServiceRegistry.instanceCount}</Text>
    </Box>
  );
}
