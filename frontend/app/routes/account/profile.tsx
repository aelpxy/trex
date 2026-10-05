import { MIN_PASSWORD_LENGTH, PasswordForm } from "~/components/account/password-form";
import { ProfileForm } from "~/components/account/profile-form";
import { Section } from "~/components/account/section";

export default function AccountProfile() {
  return (
    <>
      <Section title="Profile">
        <ProfileForm />
      </Section>
      <Section title="Password" description={`At least ${MIN_PASSWORD_LENGTH} characters. Changing it signs out your other devices.`}>
        <PasswordForm />
      </Section>
    </>
  );
}
