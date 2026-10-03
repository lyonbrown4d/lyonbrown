import { types } from "node:util";

import { KernelRuntimeError } from "./errors.js";
import {
  assertByteLimit,
  limitsFromBudget,
  sanitizeBoundedText,
  sanitizeJson,
  type SanitizationLimits,
} from "./sanitization.js";
import type { KernelJsonValue, KernelToolDefinition, PiKernelSessionOptions } from "./types.js";

export function normalizeTools(
  tools: readonly KernelToolDefinition[],
  options: PiKernelSessionOptions,
): readonly KernelToolDefinition[] {
  if (types.isProxy(tools) || !Array.isArray(tools)) {
    throw new TypeError("Tool registry must return a non-Proxy array");
  }
  if (tools.length > options.budget.maxCollectionEntries) {
    throw new KernelRuntimeError("INPUT_BUDGET_EXCEEDED", "Tool registry exceeds resource budget");
  }
  const schemaLimits = {
    ...limitsFromBudget(options.budget),
    maxStringBytes: options.budget.maxInputBytes,
  };
  const names = new Set<string>();
  const normalized: KernelToolDefinition[] = [];
  for (let index = 0; index < tools.length; index += 1) {
    const descriptor = Object.getOwnPropertyDescriptor(tools, String(index));
    if (!descriptor || !("value" in descriptor)) {
      throw new TypeError("Tool registry array accessors and holes are not accepted");
    }
    normalized.push(normalizeTool(descriptor.value, options, schemaLimits, names));
  }
  return normalized;
}

function normalizeTool(
  value: unknown,
  options: PiKernelSessionOptions,
  schemaLimits: SanitizationLimits,
  names: Set<string>,
): KernelToolDefinition {
  const fields = toolFields(value);
  const name = sanitizeIdentity(fields.name, options.budget.maxInputBytes, "Tool name");
  if (names.has(name)) throw new TypeError("Tool names must be unique");
  names.add(name);
  if (types.isProxy(fields.permissions) || !Array.isArray(fields.permissions)) {
    throw new TypeError("Tool permissions must be a non-Proxy string array");
  }
  if (fields.permissions.length > options.budget.maxCollectionEntries) {
    throw new KernelRuntimeError(
      "INPUT_BUDGET_EXCEEDED",
      "Tool permissions exceed resource budget",
    );
  }
  const permissionsValue = sanitizeJson(fields.permissions, schemaLimits);
  if (!Array.isArray(permissionsValue) || !permissionsValue.every(isString)) {
    throw new TypeError("Tool permissions must be a string array");
  }
  const permissions = permissionsValue.map((permission) =>
    sanitizeIdentity(permission, options.budget.maxInputBytes, "Permission"),
  );
  const inputSchema = sanitizeJson(fields.inputSchema, schemaLimits);
  if (!isJsonObject(inputSchema)) throw new TypeError("Tool schema must be an object");
  assertByteLimit(
    JSON.stringify(inputSchema),
    options.budget.maxInputBytes,
    "INPUT_BUDGET_EXCEEDED",
  );
  return {
    name,
    label: sanitizeBoundedText(fields.label, options.budget.maxInputBytes),
    description: sanitizeBoundedText(fields.description, options.budget.maxInputBytes),
    permissions,
    inputSchema,
    risk: fields.risk as KernelToolDefinition["risk"],
    execute: fields.execute,
  };
}

interface ToolFields {
  readonly name: string;
  readonly label: string;
  readonly description: string;
  readonly inputSchema: unknown;
  readonly permissions: unknown;
  readonly risk: unknown;
  readonly execute: KernelToolDefinition["execute"];
}

function toolFields(value: unknown): ToolFields {
  if (typeof value !== "object" || value === null || types.isProxy(value)) {
    throw new TypeError("Tool definition Proxy values are not accepted");
  }
  const prototype = Object.getPrototypeOf(value) as object | null;
  if (prototype !== Object.prototype && prototype !== null) {
    throw new TypeError("Tool definition must be a plain object");
  }
  if (Object.getOwnPropertySymbols(value).length > 0) {
    throw new TypeError("Tool definition symbol keys are not accepted");
  }
  const descriptors = Object.getOwnPropertyDescriptors(value);
  if (Object.values(descriptors).some((descriptor) => "get" in descriptor || "set" in descriptor)) {
    throw new TypeError("Tool definition accessors are not accepted");
  }
  const name = dataValue(descriptors, "name");
  const label = dataValue(descriptors, "label");
  const description = dataValue(descriptors, "description");
  const execute = dataValue(descriptors, "execute");
  if (typeof name !== "string" || typeof label !== "string" || typeof description !== "string") {
    throw new TypeError("Tool text metadata must be strings");
  }
  if (typeof execute !== "function" || types.isProxy(execute)) {
    throw new TypeError("Tool execute must be a non-Proxy function");
  }
  return {
    name,
    label,
    description,
    inputSchema: dataValue(descriptors, "inputSchema"),
    permissions: dataValue(descriptors, "permissions"),
    risk: dataValue(descriptors, "risk"),
    execute: execute as KernelToolDefinition["execute"],
  };
}

function dataValue(
  descriptors: Readonly<Record<string, PropertyDescriptor>>,
  key: string,
): unknown {
  const descriptor = descriptors[key];
  if (!descriptor || !("value" in descriptor)) throw new TypeError(`Tool ${key} is missing`);
  return descriptor.value;
}

function sanitizeIdentity(value: string, maximumBytes: number, label: string): string {
  const sanitized = sanitizeBoundedText(value, maximumBytes);
  if (sanitized !== value || sanitized.length === 0) throw new TypeError(`${label} is invalid`);
  return sanitized;
}

function isJsonObject(value: KernelJsonValue): value is Readonly<Record<string, KernelJsonValue>> {
  return value !== null && typeof value === "object" && !Array.isArray(value);
}

function isString(value: KernelJsonValue): value is string {
  return typeof value === "string";
}
