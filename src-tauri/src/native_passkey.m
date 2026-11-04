#import <AppKit/AppKit.h>
#import <AuthenticationServices/AuthenticationServices.h>
#import <Foundation/Foundation.h>
#import <Security/SecTask.h>
#import <string.h>
#import <stdatomic.h>
#import <stdint.h>

typedef void (*GatePasskeyCallback)(void *context, const char *json);

// The map and all request/delegate state are accessed on the main queue only.
static NSMutableDictionary<NSNumber *, id> *gatePasskeyRequests;
static _Atomic(uint64_t) gatePasskeyNextToken = 1;
static BOOL gatePasskeyShuttingDown;

static NSString *GateBase64(NSData *data) {
    return data ? [data base64EncodedStringWithOptions:0] : nil;
}

// This checks the running signature only. Apple still validates the AASA,
// account and device when an authorization is attempted.
int hellas_native_passkey_available(void) {
    if (@available(macOS 13.0, *)) {
        SecTaskRef task = SecTaskCreateFromSelf(kCFAllocatorDefault);
        if (!task) return 0;
        CFTypeRef value = SecTaskCopyValueForEntitlement(task, CFSTR("com.apple.developer.associated-domains"), NULL);
        CFRelease(task);
        if (!value) return 0;
        BOOL allowed = CFGetTypeID(value) == CFArrayGetTypeID() &&
            CFArrayContainsValue((CFArrayRef)value, CFRangeMake(0, CFArrayGetCount((CFArrayRef)value)), CFSTR("webcredentials:hellas.ai"));
        CFRelease(value);
        return allowed ? 1 : 0;
    }
    return 0;
}

static NSData *GateDecode(id value) {
    if (![value isKindOfClass:[NSString class]] || [(NSString *)value length] == 0) {
        return nil;
    }
    NSData *decoded = [[NSData alloc] initWithBase64EncodedString:value options:0];
    return decoded.length ? decoded : nil;
}

@interface GatePasskeyRequest : NSObject <ASAuthorizationControllerDelegate, ASAuthorizationControllerPresentationContextProviding>
@property(nonatomic) uint64_t token;
@property(nonatomic) GatePasskeyCallback callback;
@property(nonatomic) void *context;
@property(nonatomic, strong) ASAuthorizationController *controller;
@property(nonatomic, strong) NSWindow *window;
@property(nonatomic) BOOL completed;
- (void)finish:(NSDictionary *)result;
@end

@implementation GatePasskeyRequest
- (void)finish:(NSDictionary *)result {
    if (self.completed) return;
    // The registry can hold the delegate's only strong reference.
    __attribute__((objc_precise_lifetime)) GatePasskeyRequest *keepAlive = self;
    (void)keepAlive;
    self.completed = YES;
    // The registry may own the last strong reference to this delegate. Copy
    // the callback before removing it and do not access self afterwards.
    GatePasskeyCallback callback = self.callback;
    void *context = self.context;
    [gatePasskeyRequests removeObjectForKey:@(self.token)];
    NSData *bytes = [NSJSONSerialization dataWithJSONObject:result options:0 error:NULL];
    NSString *json = bytes ? [[NSString alloc] initWithData:bytes encoding:NSUTF8StringEncoding] : nil;
    callback(context, json ? json.UTF8String : "{\"error\":\"Could not encode passkey result\"}");
}

- (ASPresentationAnchor)presentationAnchorForAuthorizationController:(ASAuthorizationController *)controller {
    (void)controller;
    return self.window;
}

- (void)authorizationController:(ASAuthorizationController *)controller didCompleteWithAuthorization:(ASAuthorization *)authorization {
    (void)controller;
    id credential = authorization.credential;
    if (@available(macOS 13.0, *)) {
        if ([credential isKindOfClass:[ASAuthorizationPlatformPublicKeyCredentialRegistration class]]) {
            ASAuthorizationPlatformPublicKeyCredentialRegistration *registration = credential;
            NSString *identifier = GateBase64(registration.credentialID);
            NSString *clientData = GateBase64(registration.rawClientDataJSON);
            NSString *attestation = GateBase64(registration.rawAttestationObject);
            if (identifier.length && clientData.length && attestation.length) {
                [self finish:@{@"credential_id": identifier, @"client_data_json": clientData, @"attestation_object": attestation}];
                return;
            }
        } else if ([credential isKindOfClass:[ASAuthorizationPlatformPublicKeyCredentialAssertion class]]) {
            ASAuthorizationPlatformPublicKeyCredentialAssertion *assertion = credential;
            NSString *identifier = GateBase64(assertion.credentialID);
            NSString *clientData = GateBase64(assertion.rawClientDataJSON);
            NSString *authenticatorData = GateBase64(assertion.rawAuthenticatorData);
            NSString *signature = GateBase64(assertion.signature);
            if (identifier.length && clientData.length && authenticatorData.length && signature.length) {
                [self finish:@{@"credential_id": identifier, @"client_data_json": clientData, @"authenticator_data": authenticatorData, @"signature": signature}];
                return;
            }
        }
    }
    [self finish:@{@"error": @"AuthenticationServices returned an incomplete or unexpected credential"}];
}

- (void)authorizationController:(ASAuthorizationController *)controller didCompleteWithError:(NSError *)error {
    (void)controller;
    [self finish:@{@"error": error.localizedDescription ?: @"Passkey authorization failed"}];
}
@end

uint64_t hellas_native_passkey_start(const char *request_json, GatePasskeyCallback callback, void *context) {
    if (!callback) return 0;
    uint64_t token = atomic_fetch_add_explicit(&gatePasskeyNextToken, 1, memory_order_relaxed);
    if (!token) token = atomic_fetch_add_explicit(&gatePasskeyNextToken, 1, memory_order_relaxed);
    // Copy synchronously: the caller may release its input immediately on return.
    size_t length = request_json ? strnlen(request_json, 65537) : 0;
    NSString *request = length > 0 && length <= 65536
        ? [[NSString alloc] initWithBytes:request_json length:length encoding:NSUTF8StringEncoding] : nil;
    dispatch_async(dispatch_get_main_queue(), ^{
        GatePasskeyRequest *operation = [GatePasskeyRequest new];
        operation.token = token;
        operation.callback = callback;
        operation.context = context;
        if (!gatePasskeyRequests) gatePasskeyRequests = [NSMutableDictionary new];
        gatePasskeyRequests[@(token)] = operation;
        if (gatePasskeyShuttingDown) {
            [operation finish:@{@"error": @"Gate is shutting down"}];
            return;
        }
        if (@available(macOS 13.0, *)) {
            NSData *data = [request dataUsingEncoding:NSUTF8StringEncoding];
            id parsed = data ? [NSJSONSerialization JSONObjectWithData:data options:0 error:NULL] : nil;
            if (![parsed isKindOfClass:[NSDictionary class]]) {
                [operation finish:@{@"error": @"Invalid passkey request"}];
                return;
            }
            NSDictionary *input = parsed;
            NSString *kind = input[@"kind"];
            NSData *challenge = GateDecode(input[@"challenge"]);
            if (![kind isKindOfClass:[NSString class]] || !challenge || challenge.length > 1024) {
                [operation finish:@{@"error": @"Invalid passkey request challenge or kind"}];
                return;
            }
            NSWindow *window = NSApp.keyWindow ?: NSApp.mainWindow;
            if (!window) {
                [operation finish:@{@"error": @"No active Gate window is available for passkey authorization"}];
                return;
            }
            operation.window = window;
            ASAuthorizationPlatformPublicKeyCredentialProvider *provider = [[ASAuthorizationPlatformPublicKeyCredentialProvider alloc] initWithRelyingPartyIdentifier:@"hellas.ai"];
            ASAuthorizationRequest *authorizationRequest;
            if ([kind isEqualToString:@"register"]) {
                NSString *name = input[@"name"];
                NSData *userID = GateDecode(input[@"user_id"]);
                if (![name isKindOfClass:[NSString class]] || !name.length || name.length > 256 || !userID || userID.length > 64) {
                    [operation finish:@{@"error": @"Invalid passkey registration user"}];
                    return;
                }
                ASAuthorizationPlatformPublicKeyCredentialRegistrationRequest *registration = [provider createCredentialRegistrationRequestWithChallenge:challenge name:name userID:userID];
                registration.userVerificationPreference = ASAuthorizationPublicKeyCredentialUserVerificationPreferenceRequired;
                authorizationRequest = registration;
            } else if ([kind isEqualToString:@"assert"]) {
                NSData *identifier = GateDecode(input[@"credential_id"]);
                if (!identifier || identifier.length > 1024) {
                    [operation finish:@{@"error": @"Invalid passkey credential identifier"}];
                    return;
                }
                ASAuthorizationPlatformPublicKeyCredentialAssertionRequest *assertion = [provider createCredentialAssertionRequestWithChallenge:challenge];
                assertion.userVerificationPreference = ASAuthorizationPublicKeyCredentialUserVerificationPreferenceRequired;
                assertion.allowedCredentials = @[[[ASAuthorizationPlatformPublicKeyCredentialDescriptor alloc] initWithCredentialID:identifier]];
                authorizationRequest = assertion;
            } else {
                [operation finish:@{@"error": @"Unknown passkey operation"}];
                return;
            }
            operation.controller = [[ASAuthorizationController alloc] initWithAuthorizationRequests:@[authorizationRequest]];
            operation.controller.delegate = operation;
            operation.controller.presentationContextProvider = operation;
            [operation.controller performRequests];
        } else {
            [operation finish:@{@"error": @"Native passkeys require macOS 13 or later"}];
        }
    });
    return token;
}

void hellas_native_passkey_cancel(uint64_t token) {
    if (!token) return;
    dispatch_async(dispatch_get_main_queue(), ^{
        GatePasskeyRequest *operation = gatePasskeyRequests[@(token)];
        if (!operation || operation.completed) return;
        ASAuthorizationController *controller = operation.controller;
        [operation finish:@{@"error": @"Passkey authorization cancelled"}];
        if (@available(macOS 13.0, *)) [controller cancel];
    });
}

void hellas_native_passkey_shutdown(void) {
    dispatch_async(dispatch_get_main_queue(), ^{
        gatePasskeyShuttingDown = YES;
        for (GatePasskeyRequest *operation in [gatePasskeyRequests.allValues copy]) {
            ASAuthorizationController *controller = operation.controller;
            [operation finish:@{@"error": @"Gate is shutting down"}];
            if (@available(macOS 13.0, *)) [controller cancel];
        }
    });
}
