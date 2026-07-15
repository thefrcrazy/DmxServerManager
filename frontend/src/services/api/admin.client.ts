import { z } from "zod";
import type { operations } from "../../generated/api";
import {
    InstanceGrantSchema,
    ManagedRoleSchema,
    ManagedUserSchema,
    PermissionDescriptionSchema,
    SuccessResponseSchema,
    type InstancePermissionId,
} from "@/schemas/api";
import { BaseClient } from "./base.client";

export type CreateRoleInput = operations["createRole"]["requestBody"]["content"]["application/json"];

export type UpdateRoleInput = operations["updateRole"]["requestBody"]["content"]["application/json"];

export type CreateUserInput = operations["createUser"]["requestBody"]["content"]["application/json"];

export interface UpdateUserInput {
    role_id?: string;
    is_active?: boolean;
    language?: "fr" | "en";
    accent_color?: string;
    password?: string;
}

export class AdminClient extends BaseClient {
    listPermissions() {
        return this.request("/permissions", z.array(PermissionDescriptionSchema));
    }

    listRoles() {
        return this.request("/roles", z.array(ManagedRoleSchema));
    }

    createRole(input: CreateRoleInput) {
        return this.request("/roles", ManagedRoleSchema, {
            method: "POST",
            body: JSON.stringify(input),
        });
    }

    updateRole(id: string, input: UpdateRoleInput) {
        return this.request(`/roles/${encodeURIComponent(id)}`, ManagedRoleSchema, {
            method: "PATCH",
            body: JSON.stringify(input),
        });
    }

    deleteRole(id: string) {
        return this.request(`/roles/${encodeURIComponent(id)}`, SuccessResponseSchema, {
            method: "DELETE",
        });
    }

    listUsers() {
        return this.request("/users", z.array(ManagedUserSchema));
    }

    createUser(input: CreateUserInput) {
        return this.request("/users", ManagedUserSchema, {
            method: "POST",
            body: JSON.stringify(input),
        });
    }

    updateUser(id: string, input: UpdateUserInput) {
        return this.request(`/users/${encodeURIComponent(id)}`, ManagedUserSchema, {
            method: "PATCH",
            body: JSON.stringify(input),
        });
    }

    listGrants(userId: string) {
        return this.request(
            `/users/${encodeURIComponent(userId)}/instances`,
            z.array(InstanceGrantSchema),
        );
    }

    setGrant(userId: string, instanceId: string, permissions: InstancePermissionId[]) {
        return this.request(
            `/users/${encodeURIComponent(userId)}/instances/${encodeURIComponent(instanceId)}`,
            InstanceGrantSchema,
            { method: "PUT", body: JSON.stringify({ permissions }) },
        );
    }

    deleteGrant(userId: string, instanceId: string) {
        return this.request(
            `/users/${encodeURIComponent(userId)}/instances/${encodeURIComponent(instanceId)}`,
            SuccessResponseSchema,
            { method: "DELETE" },
        );
    }
}
